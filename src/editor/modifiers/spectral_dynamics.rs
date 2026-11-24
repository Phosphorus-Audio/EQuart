use super::*;
use crate::dynamics::DetectorInput;
use crate::region::dynamics::{Control, Curve, Detector, Mode};
use nice_plug::prelude::Enum;

mod level_scale;
mod monitor;
mod transfer;
use level_scale::LevelScale;

pub(in crate::editor) const HEIGHT: f32 = 180.0;
const MONITOR: f32 = 230.0;
const CONTROLS: f32 = 145.0;
pub(super) const COMPACT: f32 = HEADER_WIDTH + HEIGHT + CONTROLS;
pub(super) const WIDTH: f32 = COMPACT + MONITOR;

impl EquartEditor {
    pub(in crate::editor) fn reset_region_dynamics(&self, setter: &ParamSetter<'_>, index: usize) {
        let params = &self.params.regions[index].dynamics;
        self.set(setter, &params.enabled, false);
        self.set(setter, &params.bypass, false);
        self.set(setter, &params.mode, Mode::Compressor);
        self.set(setter, &params.detector, Detector::Rms);
        self.set(setter, &params.source, DetectorInput::Main);
        self.params
            .spectral_transfer_curves
            .edit(index, Curve::default());

        for param in [
            &params.threshold,
            &params.ratio,
            &params.attack,
            &params.release,
            &params.knee,
            &params.makeup,
            &params.range,
            &params.link,
        ] {
            let default = param.default_plain_value();
            if param.value() != default {
                self.set(setter, param, default);
            }
        }
    }

    pub(super) fn spectral_dynamics_modifier(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        index: usize,
    ) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let params = &self.params.regions[index].dynamics;
        let mut control = params.control();
        let curve_id = ui.make_persistent_id(("transfer-curve", index));
        let curve = self.params.spectral_transfer_curves.read(index);
        control.curve = ui.data_mut(|data| {
            if let Some(curve) = curve {
                data.insert_temp(curve_id, curve);
                curve
            } else {
                data.get_temp::<Curve>(curve_id).unwrap_or_default()
            }
        });

        let accent = self.theme.region[index];
        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let (toggle, remove) =
            modifier_header(ui, header, index, "DYNAMICS", !control.bypass, accent);

        if toggle {
            self.set(&setter, &params.bypass, !control.bypass);
        }

        if remove {
            self.set(&setter, &params.enabled, false);
            return;
        }

        let compact = self.region_dynamics_compact[index];
        let mut paused = self.region_dynamics_paused[index];
        let latest = self.telemetry.region_dynamics[index].latest();
        let transfer = Rect::from_min_size(header.right_top(), Vec2::splat(rect.height()));
        let charts = Rect::from_min_size(
            transfer.min,
            Vec2::new(
                transfer.width() + if compact { 0.0 } else { MONITOR },
                transfer.height(),
            ),
        );
        let scale_id = egui::Id::unique(("spectral-dynamics-level-scale", index));
        let mut scale = ui
            .data(|data| data.get_temp::<LevelScale>(scale_id))
            .unwrap_or_default();
        scale = scale.scroll(ui, charts, scale_id.with("wheel"));
        self.transfer_curve(ui, transfer, index, control, latest, scale);

        let mut right = transfer.right();
        if !compact {
            let monitor =
                Rect::from_min_size(transfer.right_top(), Vec2::new(MONITOR, rect.height()));
            self.dynamics_monitor(ui, monitor, index, &mut paused, scale);
            right = monitor.right();
        }

        let controls =
            Rect::from_min_max(Pos2::new(right, rect.top()), rect.max).shrink2(Vec2::new(8.0, 7.0));
        let mut next_compact = compact;
        let custom = control.curve.is_custom();
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("spectral-dynamics", index))
                .max_rect(controls)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_width(controls.width());
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 3.0);
                if custom {
                    self.spectral_number(ui, &setter, "Range", &params.range, 0.1);
                    self.spectral_number(ui, &setter, "Makeup", &params.makeup, 0.1);
                } else {
                    self.spectral_number(ui, &setter, "Threshold", &params.threshold, 0.1);
                    self.spectral_number(ui, &setter, "Ratio", &params.ratio, 0.04);
                }
                self.spectral_number(ui, &setter, "Attack", &params.attack, 0.3);
                self.spectral_number(ui, &setter, "Release", &params.release, 1.0);

                let size = Vec2::new(controls.width(), 24.0);
                let show = if compact { "Expand" } else { "Compact" };
                if ui.add_sized(size, egui::Button::new(show)).clicked() {
                    next_compact = !compact;
                }

                let advanced = ui.add_sized(size, egui::Button::new("Settings"));
                egui::Popup::menu(&advanced)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .frame(menu::card(ui.ctx()))
                    .width(238.0)
                    .show(|ui| {
                        scale = scale.settings(ui);
                        ui.separator();
                        self.spectral_settings(ui, &setter, index, control);
                    });

                if let Some(level) = latest {
                    let status = if level.external_missing {
                        "No sidechain".to_owned()
                    } else {
                        format!("In {:.1}   GR {:+.1}", level.input, level.reduction)
                    };
                    let status = ui.small(status);
                    if !compact && !paused {
                        status.on_hover_text(format!("Output {:.1} dB", level.output));
                    }
                }
            },
        );

        self.region_dynamics_compact[index] = next_compact;
        self.region_dynamics_paused[index] = paused;
        ui.data_mut(|data| data.insert_temp(scale_id, scale));
    }

    fn spectral_settings(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        index: usize,
        control: Control,
    ) {
        let params = &self.params.regions[index].dynamics;
        let custom = control.curve.is_custom();

        ui.horizontal(|ui| {
            self.settings_label(ui, "Transfer");
            if self.settings_button(ui, "Standard", !custom).clicked() && custom {
                self.params
                    .spectral_transfer_curves
                    .edit(index, control.curve.with_enabled(false));
            }
            if self.settings_button(ui, "Custom", custom).clicked() && !custom {
                let curve = if control.curve.points().is_empty() {
                    Curve::from_standard(control)
                } else {
                    control.curve.with_enabled(true)
                };
                self.params.spectral_transfer_curves.edit(index, curve);
            }
        });
        ui.add_enabled_ui(!custom, |ui| {
            self.settings_choice(
                ui,
                setter,
                "Mode",
                &params.mode,
                [
                    (Mode::Compressor, "Compressor"),
                    (Mode::Expander, "Expander"),
                ],
            )
            .on_disabled_hover_text("Switch to Standard to use the threshold and ratio curve.");
            self.spectral_number(ui, setter, "Knee", &params.knee, 0.1);
        });
        ui.separator();
        self.settings_choice(
            ui,
            setter,
            "Detector",
            &params.detector,
            [(Detector::Peak, "Peak"), (Detector::Rms, "RMS")],
        )
        .on_hover_text("Peak follows transients. RMS follows average energy.");
        self.settings_choice(
            ui,
            setter,
            "Source",
            &params.source,
            [
                (DetectorInput::Main, "Region"),
                (DetectorInput::Sidechain, "Sidechain"),
            ],
        )
        .on_hover_text("Detect the region in the main input or the external sidechain.");
        ui.separator();
        self.spectral_number(ui, setter, "Makeup", &params.makeup, 0.1);
        self.spectral_number(ui, setter, "Range", &params.range, 0.1);
        self.spectral_number(ui, setter, "Stereo Link", &params.link, 0.5);
    }

    fn settings_choice<T: Enum + Copy + PartialEq + 'static>(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        label: &str,
        param: &EnumParam<T>,
        options: [(T, &str); 2],
    ) -> Response {
        ui.horizontal(|ui| {
            self.settings_label(ui, label);

            for (value, text) in options {
                if self
                    .settings_button(ui, text, param.value() == value)
                    .clicked()
                {
                    self.set(setter, param, value);
                }
            }
        })
        .response
    }

    fn settings_label(&self, ui: &mut egui::Ui, label: &str) {
        ui.allocate_ui_with_layout(
            Vec2::new(60.0, 24.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.colored_label(self.theme.text.muted, label);
            },
        );
    }

    fn settings_button(&self, ui: &mut egui::Ui, label: &str, selected: bool) -> Response {
        let accent = self.theme.panel.accent;
        let edge = if selected {
            accent
        } else {
            self.theme.text.muted.gamma_multiply(0.44)
        };
        ui.add(
            egui::Button::new(label)
                .min_size(Vec2::new(76.0, 24.0))
                .selected(selected)
                .stroke(Stroke::new(1.0, edge)),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn spectral_number(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        label: &str,
        param: &FloatParam,
        speed: f32,
    ) {
        let response = self.dynamic_number(ui, setter, label, param, speed);
        if response.clicked() && ui.input(|input| input.modifiers.command) {
            self.set(setter, param, param.default_plain_value());
        }

        if response.hovered() {
            let (delta, fine) =
                ui.input(|input| (input.smooth_scroll_delta.y, input.modifiers.shift));
            if delta != 0.0 {
                let scale = if fine { 0.0001 } else { 0.001 };
                let value = (param.unmodulated_normalized_value() + delta * scale).clamp(0.0, 1.0);
                self.set(setter, param, param.preview_plain(value));
            }
        }
    }
}
