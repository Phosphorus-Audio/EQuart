use super::*;
use crate::dynamics::{DetectorInput, DetectorMode, DetectorParams};

impl EquartEditor {
    pub(super) fn dynamic_modifier(&self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let Some(gui) = &self.gui else { return };

        let setter = gui.param_setter();
        let band = self.params.band(index);
        let dynamic = &band.dynamics;
        let (level, offset) = self.telemetry.dynamic_meter(index);
        let accent = POINT_COLORS[index];
        let divider = Stroke::new(1.0, Color32::from_white_alpha(18));

        ui.painter().vline(rect.left(), rect.y_range(), divider);

        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let mut inner = rect.shrink2(Vec2::new(10.0, 7.0));

        inner.min.x += HEADER_WIDTH;

        let bypassed = dynamic.bypassed.value();
        let (toggle, remove) = modifier_header(ui, header, index, "DYNAMIC", !bypassed, accent);

        if toggle {
            self.set(&setter, &dynamic.bypassed, !bypassed);
        }

        if remove {
            self.set(&setter, &dynamic.enabled, false);
            return;
        }

        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("dynamic-modifier", index))
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),

            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                ui.visuals_mut().selection.stroke.color = accent;
                let (knob_rect, _) = ui.allocate_exact_size(Vec2::new(THRESHOLD_WIDTH, 100.0), Sense::hover());
                knob::knob(
                    ui,
                    &dynamic.threshold,
                    &setter,
                    knob::KnobAppearance {
                        center: knob_rect.center(),
                        radius: 22.0,
                        grow: 1.05,
                        body_fill: 1.06,
                        label_gap: 32.0,
                        label: "THRESHOLD",
                        bipolar: false,
                    },
                    knob::KnobMode::Metered(knob::KnobMeter { level, gain: offset }),
                ).on_hover_text(format!(
                    "Detector level {}\nDynamic gain {offset:+.1} dB",
                    level.map_or_else(|| "—".to_owned(), |level| format!("{level:.1} dB")),
                ));

                column(ui, 120.0, |ui| {
                    ui.colored_label(accent, format!("Gain {offset:+.1} dB"));
                    self.dynamic_number(ui, &setter, "Attack", &dynamic.attack, 0.2);
                    self.dynamic_number(ui, &setter, "Release", &dynamic.release, 0.5);
                });

                for (name, enabled, range, ratio) in [
                    ("Above", &dynamic.above_enabled, &dynamic.range, &dynamic.above_ratio),
                    ("Below", &dynamic.below_enabled, &dynamic.below_range, &dynamic.below_ratio),
                ] {
                    column(ui, 98.0, |ui| {
                        let mut on = enabled.value();
                        if ui.checkbox(&mut on, name).changed() {
                            self.set(&setter, enabled, on);
                        }
                        self.dynamic_number(ui, &setter, "Range", range, 0.15);
                        self.dynamic_number(ui, &setter, "Ratio", ratio, 0.5);
                    });
                }

                column(ui, 138.0, |ui| self.detector_controls(ui, &setter, "Detect", &dynamic.detect));
                column(ui, 138.0, |ui| self.detector_controls(ui, &setter, "Relative", &dynamic.relative));
                column(ui, 139.0, |ui| {
                    ui.strong("Mix");
                    self.dynamic_number(ui, &setter, "Relative", &dynamic.relative_amount, 0.5)
                        .on_hover_text("Blend absolute RMS with bandwidth-matched relative level. Equal noise density is −24 dB on the threshold scale.");
                    self.dynamic_number(ui, &setter, "Onsets", &dynamic.onsets_amount, 0.5)
                        .on_hover_text("Blend level detection with the positive difference between fast and slow RMS power. Steady signals contribute little onset energy.");
                    if !band.kind.value().has_gain() {
                        ui.small("This filter has no Gain");
                    }
                });
            },
        );
    }

    fn detector_controls(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        title: &str,
        params: &DetectorParams,
    ) {
        ui.strong(title);
        let input = |source| match source {
            DetectorInput::Main => "Main input",
            DetectorInput::Sidechain => "Sidechain",
        };

        let sources = [DetectorInput::Main, DetectorInput::Sidechain];
        let current_input = params.input.value();
        let combo = egui::ComboBox::from_id_salt((title, "input"))
            .width(117.0)
            .selected_text(input(current_input))
            .show_ui(ui, |ui| {
                for source in sources {
                    let choice = ui.selectable_label(current_input == source, input(source));
                    if choice.clicked() {
                        self.set(setter, &params.input, source);
                    }
                    if source == DetectorInput::Sidechain {
                        choice.on_hover_text("Route the host's sidechain bus to this detector. Without a sidechain bus, the modifier releases to 0 dB when this input is needed.");
                    }
                }
            });

        if let Some(source) = crate::editor::menu::wheel_choice(
            &combo.response,
            combo.inner.is_none(),
            current_input,
            &sources,
        ) {
            self.set(setter, &params.input, source);
        }

        let mode_label = |mode| match mode {
            DetectorMode::Follow => "Follow",
            DetectorMode::Free => "Free",
            DetectorMode::Broadband => "Broadband",
            DetectorMode::BandShape => "Band shape",
        };

        let mode = params.mode.value();

        let modes = [
            DetectorMode::Follow,
            DetectorMode::Free,
            DetectorMode::Broadband,
            DetectorMode::BandShape,
        ];

        let combo = egui::ComboBox::from_id_salt((title, "mode"))
            .width(117.0)
            .selected_text(mode_label(mode))
            .show_ui(ui, |ui| {
                for choice in modes {
                    if ui
                        .selectable_label(mode == choice, mode_label(choice))
                        .clicked()
                    {
                        self.set(setter, &params.mode, choice);
                    }
                }
            });

        if let Some(choice) =
            crate::editor::menu::wheel_choice(&combo.response, combo.inner.is_none(), mode, &modes)
        {
            self.set(setter, &params.mode, choice);
        }

        ui.add_enabled_ui(mode == DetectorMode::Free, |ui| {
            self.dynamic_number(ui, setter, "Freq", &params.frequency, 2.0);
            self.dynamic_number(ui, setter, "Q", &params.q, 0.02);
        });
    }
}
