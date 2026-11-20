use super::*;
use crate::dynamics::DetectorMode;
use crate::params::BandParams;
use crate::pitch::{DetectionMode, DetectionRange, PitchDetectionParams, TargetMode};

mod dynamics;
mod harmonic;

const HEADER_WIDTH: f32 = 36.0;
const HEADER_BUTTON_OFFSET: f32 = 43.0;
const HEADER_ICON_SIZE: f32 = 14.0;
const THRESHOLD_WIDTH: f32 = 78.0;
const COLUMN_GAP: f32 = 10.0;
const DYNAMIC_WIDTH: f32 = 808.0 + HEADER_WIDTH + THRESHOLD_WIDTH + COLUMN_GAP;
const PITCH_WIDTH: f32 = HEADER_WIDTH + 2.0 * 10.0 + 160.0 + 135.0 + 145.0 + 2.0 * COLUMN_GAP;
const RESONANCE_WIDTH: f32 = HEADER_WIDTH + 2.0 * 10.0 + 145.0 + 140.0 + 140.0 + 2.0 * COLUMN_GAP;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Modifier {
    Dynamic,
    Pitch,
    Harmonic,
    Resonance,
}

impl Modifier {
    pub(super) const BAND: [Self; 2] = [Self::Dynamic, Self::Pitch];
    pub(super) const REGION: [Self; 2] = [Self::Harmonic, Self::Resonance];

    pub(super) fn attached_to_region(self, region: &crate::region::RegionParams) -> bool {
        match self {
            Self::Harmonic => region.harmonic.enabled.value(),
            Self::Resonance => region.resonance.enabled.value(),
            _ => false,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Dynamic => "Dynamic",
            Self::Pitch => "f0lock",
            Self::Harmonic => "Harmonic",
            Self::Resonance => "Resonance",
        }
    }

    pub(super) fn attached(self, band: &BandParams) -> bool {
        match self {
            Self::Dynamic => band.dynamics.enabled.value(),
            Self::Pitch => band.pitch.enabled.value(),
            Self::Harmonic | Self::Resonance => false,
        }
    }

    pub(super) fn width(self) -> f32 {
        match self {
            Self::Dynamic => DYNAMIC_WIDTH,
            Self::Pitch => PITCH_WIDTH,
            Self::Harmonic => harmonic::width(crate::region::FundamentalSource::Tracked),
            Self::Resonance => RESONANCE_WIDTH,
        }
    }

    pub(super) fn region_width(self, region: &crate::region::RegionParams) -> f32 {
        match self {
            Self::Harmonic => harmonic::width(region.harmonic.source.value()),
            _ => self.width(),
        }
    }

    fn unavailable(self, band: &BandParams) -> Option<&'static str> {
        if self.attached(band) {
            Some("Already attached to this band.")
        } else if self == Self::Dynamic && !band.kind.value().has_gain() {
            Some("Dynamic requires a filter with Gain.")
        } else {
            None
        }
    }

    pub(super) fn available_on(band: &BandParams) -> bool {
        Self::BAND
            .iter()
            .any(|modifier| modifier.unavailable(band).is_none())
    }

    pub(super) fn show(self, editor: &mut EquartEditor, ui: &mut egui::Ui, rect: Rect, band: usize) {
        match self {
            Self::Dynamic => editor.dynamic_modifier(ui, rect, band),
            Self::Pitch => editor.pitch_modifier(ui, rect, band),
            Self::Harmonic => editor.harmonic_modifier(ui, rect, band),
            Self::Resonance => editor.resonance_modifier(ui, rect, band),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ModifierTarget {
    Band(usize),
    Region(usize),
}

pub(super) struct Picker {
    target: ModifierTarget,
    selected: Option<Modifier>,
    life: controls::CardLife,
    anchor: Rect,
}

impl Picker {
    pub(super) fn is_up(&self) -> bool {
        self.life.is_up()
    }
}

impl EquartEditor {
    pub(super) fn show_modifier_picker(&mut self, button: &Response, target: ModifierTarget) {
        let ctx = &button.ctx;
        let id = egui::Id::unique(("modifier-picker", target));
        let anchor = egui::PopupAnchor::from(button)
            .rect(id, ctx)
            .expect("a button popup has a rectangular anchor");
        let mut picker = self
            .modifier_picker
            .take()
            .filter(|picker| picker.target == target)
            .unwrap_or(Picker {
                target,
                selected: None,
                life: controls::CardLife::SHUT,
                anchor,
            });
        let now = ctx.input(|i| i.time);
        if picker.life.is_up() || button.clicked() {
            picker.anchor = anchor;
        }
        if button.clicked() {
            if picker.is_up() {
                picker.life.release(now);
            } else {
                picker.selected = None;
                picker.life.open(now);
            }
        }
        if !picker.life.is_shown() {
            return;
        }
        let (choices, accent) = match target {
            ModifierTarget::Band(index) => (Modifier::BAND.as_slice(), POINT_COLORS[index]),
            ModifierTarget::Region(_) => (Modifier::REGION.as_slice(), regions::REGION_COLOR),
        };
        let unavailable = |modifier: Modifier| match target {
            ModifierTarget::Band(index) => modifier.unavailable(self.params.band(index)),
            ModifierTarget::Region(index) => modifier.attached_to_region(&self.params.regions[index])
                .then_some("Already attached to this region."),
        };
        let available = |modifier: Modifier| self.gui.is_some() && unavailable(modifier).is_none();
        let popup = egui::Popup::menu(button)
            .id(id)
            .open(true)
            .anchor(picker.anchor)
            .align(egui::emath::RectAlign::TOP_END)
            .gap(4.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .width(172.0)
            .frame(menu::card());
        let mut add = false;
        let dismissed = Self::animated_card(
            popup,
            &mut picker.life,
            now,
            Some(picker.anchor.right_top()),
            |ui, _, flash, fall| {
                ui.set_width(158.0);
                menu::LabelFlash::begin(
                    ui,
                    ui.max_rect(),
                    if fall.is_none() { flash } else { 1.0 },
                );
                menu::body(ui);
                menu::section(ui, "ADD MODIFIER");
                for &modifier in choices {
                    let mut response = ui
                        .add_enabled_ui(available(modifier), |ui| {
                            menu::row_response(
                                ui,
                                modifier.label(),
                                None,
                                picker.selected == Some(modifier),
                                accent,
                            )
                        })
                        .inner;
                    if let Some(reason) = unavailable(modifier) {
                        response = response.on_disabled_hover_text(reason);
                    }
                    if response.clicked() {
                        picker.selected = Some(modifier);
                    }
                    // egui might count opener + fast double click as triple click
                    if response.double_clicked() || response.triple_clicked() {
                        picker.selected = Some(modifier);
                        add = true;
                    }
                }
                menu::divider(ui);
                let cancel = ui
                    .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 9.0;
                        add |= picker_button(
                            ui,
                            "Add",
                            40.0,
                            Some(accent),
                            picker.selected.is_some_and(available),
                        );
                        picker_button(ui, "Cancel", 48.0, None, true)
                    })
                    .inner;
                add || cancel
            },
        );
        if add
            && let Some(modifier) = picker.selected.filter(|&modifier| available(modifier))
            && let Some(gui) = &self.gui
        {
            let setter = gui.param_setter();
            let (enabled, bypassed) = match (target, modifier) {
                (ModifierTarget::Band(index), Modifier::Dynamic) => {
                    let band = self.params.band(index);
                    if band.dynamics.detect.mode.value() == DetectorMode::BandShape {
                        self.set(&setter, &band.dynamics.detect.mode, DetectorMode::Follow);
                    }
                    (&band.dynamics.enabled, &band.dynamics.bypassed)
                }
                (ModifierTarget::Band(index), Modifier::Pitch) => {
                    let pitch = &self.params.band(index).pitch;
                    (&pitch.enabled, &pitch.bypassed)
                }
                (ModifierTarget::Region(index), Modifier::Resonance) => {
                    let resonance = &self.params.regions[index].resonance;
                    (&resonance.enabled, &resonance.bypassed)
                }
                (ModifierTarget::Region(index), Modifier::Harmonic) => {
                    let harmonic = &self.params.regions[index].harmonic;
                    (&harmonic.enabled, &harmonic.bypassed)
                }
                _ => unreachable!("the picker only offers modifiers for its target"),
            };
            if bypassed.value() {
                self.set(&setter, bypassed, false);
            }
            self.set(&setter, enabled, true);
            let index = match target {
                ModifierTarget::Band(index) | ModifierTarget::Region(index) => index,
            };
            self.reveal_modifier = Some((index, modifier));
        }
        if dismissed && !button.clicked() {
            picker.life.release(now);
        }
        if picker.life.is_shown() {
            self.modifier_picker = Some(picker);
        }
    }

    fn resonance_modifier(&mut self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let region = &self.params.regions[index];
        let res = &region.resonance;
        let bypassed = res.bypassed.value();
        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let (toggle, remove) = modifier_header(ui, header, index, "RESONANCE", !bypassed, regions::REGION_COLOR);
        if toggle {
            self.set(&setter, &res.bypassed, !bypassed);
        }
        if remove {
            self.set(&setter, &res.enabled, false);
            return;
        }

        let mut inner = rect.shrink2(Vec2::new(10.0, 7.0));
        inner.min.x += HEADER_WIDTH;

        let mut held = false;

        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("resonance", index))
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                column(ui, 145.0, |ui| {
                    self.dynamic_number(ui, &setter, "Depth", &res.depth, 0.1)
                        .on_hover_text("Maximum cut per peak. Set region Gain to 0 dB for resonance suppression alone.");
                    self.dynamic_number(ui, &setter, "Prominence", &res.threshold, 0.1)
                        .on_hover_text("Minimum contrast above neighboring frequencies to trigger reduction.");
                    self.dynamic_number(ui, &setter, "Q", &res.q, 0.1)
                        .on_hover_text("Higher Q narrows each cut.");
                    self.dynamic_number(ui, &setter, "Ratio", &res.ratio, 1.0)
                        .on_hover_text("Suppression slope applied to excess above Prominence.");
                });

                column(ui, 140.0, |ui| {
                    self.dynamic_number(ui, &setter, "Attack", &res.attack, 0.25)
                        .on_hover_text("How fast cuts react to new resonances.");
                    self.dynamic_number(ui, &setter, "Release", &res.release, 1.0)
                        .on_hover_text("How fast cuts recover after a resonance drops.");
                    self.dynamic_number(ui, &setter, "Smooth", &res.smooth, 1.0)
                        .on_hover_text("Background window width: low targets sharp spikes, high catches wider mud.");
                });

                column(ui, 140.0, |ui| {
                    self.dynamic_number(ui, &setter, "Link", &res.stereo_link, 1.0)
                        .on_hover_text("Stereo link: 100% links L/R, lower values suppress single-channel resonances.");

                    self.dynamic_number(ui, &setter, "Bias", &res.bias, 1.0)
                        .on_hover_text("Frequency sensitivity tilt: negative favors lows, positive favors highs.");

                    let control = region.control();

                    let strongest = self
                        .telemetry
                        .resonance_reductions(index)
                        .into_iter()
                        .map(|cut| cut.band(control))
                        .filter(|band| band.enabled)
                        .min_by(|a, b| a.gain.total_cmp(&b.gain));
                    let active = !bypassed && !region.bypass.value() && !self.params.bypass.value();

                    ui.horizontal(|ui| {
                        let button = ui.add_enabled(
                            active,
                            egui::Button::new("Δ").sense(egui::Sense::click_and_drag()),
                        );

                        held = active
                            && (button.is_pointer_button_down_on() || button.dragged())
                            && ui.input(|i| i.focused && i.pointer.primary_down());

                        if !held {
                            button.on_hover_text("Hold to hear what this region's Resonance removes.");
                        }

                        if !active {
                            ui.label("Bypassed");
                        } else if let Some(cut) = strongest {
                            ui.label(format!("{:.0} Hz  {:.1} dB", cut.frequency, cut.gain));
                        } else {
                            ui.label("No active peaks");
                        }
                    });
                });
            },
        );
        if held {
            self.held_delta = Some(index);
        }
    }

    fn pitch_modifier(&self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let band = self.params.band(index);
        let pitch = &band.pitch;
        let accent = POINT_COLORS[index];
        let active = !pitch.bypassed.value();
        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let (toggle, remove) =
            modifier_header(ui, header, index, Modifier::Pitch.label(), active, accent);
        if toggle {
            self.set(&setter, &pitch.bypassed, active);
        }
        if remove {
            self.set(&setter, &pitch.enabled, false);
            return;
        }

        let mut inner = rect.shrink2(Vec2::new(10.0, 7.0));
        inner.min.x += HEADER_WIDTH;
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("pitch-modifier", index))
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                column(ui, 160.0, |ui| {
                    ui.strong("Detection");
                    self.pitch_detection_choices(ui, &setter, &pitch.detection);
                    let detected = (active && band.enabled.value() && !self.params.bypass.value())
                        .then(|| self.telemetry.detected_pitch(index))
                        .flatten();
                    if let Some(hz) = detected {
                        ui.colored_label(accent, format!("Detected {hz:.1} Hz"));
                    } else {
                        ui.label("No pitch detected");
                    }
                    if active && let Some(hz) = self.telemetry.tracked_frequency(index) {
                        let label = if detected.is_some() { "Band" } else { "Held" };
                        ui.label(format!("{label} {hz:.1} Hz"));
                    }
                });
                column(ui, 135.0, |ui| {
                    self.pitch_tracking_controls(
                        ui,
                        &setter,
                        &pitch.detection,
                        "Smooths note changes. The band holds its last frequency when detection is lost.",
                    );
                    self.dynamic_number(ui, &setter, "Multiplier", &pitch.multiplier, 0.01)
                        .on_hover_text(
                            "Sets the band frequency to the detected fundamental times this value.",
                        );
                });
                column(ui, 145.0, |ui| {
                    self.pitch_target_controls(ui, &setter, &pitch.detection);
                });
            },
        );
    }

    fn pitch_detection_choices(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        detection: &PitchDetectionParams,
    ) {
        self.pitch_choice(
            ui,
            setter,
            &detection.algorithm,
            &DetectionMode::ALL,
            DetectionMode::label,
        );
        self.pitch_choice(
            ui,
            setter,
            &detection.profile,
            &DetectionRange::ALL,
            DetectionRange::label,
        );
    }

    fn pitch_tracking_controls(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        detection: &PitchDetectionParams,
        time_help: &str,
    ) {
        ui.strong("Tracking");
        self.dynamic_number(ui, setter, "Time", &detection.tracking, 0.5)
            .on_hover_text(time_help);
        self.dynamic_number(ui, setter, "Gate", &detection.gate, 0.25)
            .on_hover_text("Minimum detector RMS level.");
        self.dynamic_number(ui, setter, "Clarity", &detection.clarity, 0.25)
            .on_hover_text("Minimum pitch confidence.");
    }

    fn pitch_target_controls(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        detection: &PitchDetectionParams,
    ) {
        ui.strong("Target");
        self.pitch_choice(
            ui,
            setter,
            &detection.target,
            &TargetMode::ALL,
            TargetMode::label,
        );
        match detection.target.value() {
            TargetMode::Auto => {}
            TargetMode::Range => {
                self.dynamic_number(ui, setter, "Min", &detection.minimum, 1.0);
                self.dynamic_number(ui, setter, "Max", &detection.maximum, 1.0);
            }
            TargetMode::Reference => {
                self.dynamic_number(ui, setter, "Freq", &detection.reference, 1.0);
                self.dynamic_number(ui, setter, "Tolerance", &detection.tolerance, 0.1);
            }
        }
    }

    fn pitch_choice<T: nice_plug::prelude::Enum + Copy + PartialEq + 'static>(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        param: &EnumParam<T>,
        choices: &[T],
        label: impl Fn(T) -> &'static str,
    ) {
        let current = param.value();
        egui::ComboBox::from_id_salt(param.name())
            .width(ui.available_width() - 16.0)
            .selected_text(label(current))
            .show_ui(ui, |ui| {
                for &choice in choices {
                    if ui
                        .selectable_label(current == choice, label(choice))
                        .clicked()
                    {
                        self.set(setter, param, choice);
                    }
                }
            });
    }

    fn dynamic_number(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        label: &str,
        param: &FloatParam,
        speed: f32,
    ) -> Response {
        ui.horizontal(|ui| {
            ui.label(label);
            let mut value = param.value();
            let range = param.range();
            let response = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::DragValue::new(&mut value)
                            .range(range.unnormalize(0.0)..=range.unnormalize(1.0))
                            .suffix(param.unit())
                            .speed(speed)
                            .max_decimals(3),
                    )
                })
                .inner;
            if response.changed() {
                self.set(setter, param, value);
            }
            response
        })
        .inner
    }
}

fn picker_button(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    accent: Option<Color32>,
    enabled: bool,
) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 20.0), sense);
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    };
    let hover = if enabled {
        hover_amount(&response)
    } else {
        0.0
    };
    let painter = ui.painter();
    if let Some(accent) = accent {
        let base = if enabled { 0.18 } else { 0.06 };
        painter.rect_filled(rect, 5.0, accent.gamma_multiply(base + 0.17 * hover));
        painter.rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0, accent.gamma_multiply(2.1 * base + 0.25 * hover)),
            egui::StrokeKind::Inside,
        );
    } else if hover > 0.0 {
        painter.rect_filled(
            rect,
            5.0,
            Color32::from_white_alpha(30).gamma_multiply(hover),
        );
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        if enabled { TEXT } else { MUTED },
    );
    response.clicked()
}

fn modifier_header(
    ui: &mut egui::Ui,
    rect: Rect,
    index: usize,
    name: &str,
    active: bool,
    accent: Color32,
) -> (bool, bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, Color32::from_white_alpha(7));
    painter.vline(
        rect.right(),
        rect.y_range(),
        Stroke::new(1.0, Color32::from_white_alpha(18)),
    );
    let ink = if active { TEXT } else { MUTED };
    let galley = painter.layout_no_wrap(
        name.to_owned(),
        FontId::new(10.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
        ink,
    );
    let anchor = egui::Align2::CENTER_CENTER;
    let origin = anchor.anchor_size(rect.center(), galley.size()).min;
    painter.add(
        egui::epaint::TextShape::new(origin, galley, ink)
            .with_angle_and_anchor(-std::f32::consts::FRAC_PI_2, anchor),
    );

    let button = |ui: &mut egui::Ui, offset, action, tooltip| {
        let rect =
            Rect::from_center_size(rect.center() + Vec2::new(0.0, offset), Vec2::splat(22.0));
        let response = ui
            .interact(
                rect,
                ui.make_persistent_id(("modifier-header", index, name, action)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let hover = hover_amount(&response);
        if hover > 0.0 {
            ui.painter().rect_filled(
                rect,
                5.0,
                Color32::from_white_alpha(20).gamma_multiply(hover),
            );
        }
        response.on_hover_text(tooltip)
    };
    let power = button(
        ui,
        -HEADER_BUTTON_OFFSET,
        "power",
        if active {
            "Bypass modifier"
        } else {
            "Enable modifier"
        },
    );
    controls::power_icon(
        ui.painter(),
        power.rect.center(),
        if active { lighten(accent, 0.35) } else { MUTED },
        HEADER_ICON_SIZE,
    );
    let menu_button = button(ui, HEADER_BUTTON_OFFSET, "menu", "Modifier menu");
    icons::draw(
        ui.painter(),
        ui.ctx(),
        icons::Glyph::ModifierMenu,
        Rect::from_center_size(menu_button.rect.center(), Vec2::splat(HEADER_ICON_SIZE)),
        TEXT,
    );
    let remove = egui::Popup::menu(&menu_button)
        .frame(menu::card())
        .width(144.0)
        .show(|ui| {
            let clicked = menu::row(ui, "Remove modifier", None, false, accent);
            if clicked {
                ui.close();
            }
            clicked
        })
        .is_some_and(|response| response.inner);
    (power.clicked(), remove)
}

fn column(ui: &mut egui::Ui, width: f32, draw: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            draw(ui);
        },
    );
}
