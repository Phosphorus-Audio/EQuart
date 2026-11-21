use super::*;
use crate::region::{FundamentalSource, HarmonicMode, MAX_HARMONIC};

const COLUMNS: [f32; 4] = [134.0, 132.0, 118.0, 122.0];
const HORIZONTAL_INSET: f32 = 10.0;

pub(super) const fn width(source: FundamentalSource) -> f32 {
    let [mask, fundamental, tracking, target] = COLUMNS;
    let base = HEADER_WIDTH + 2.0 * HORIZONTAL_INSET + mask + COLUMN_GAP + fundamental;
    match source {
        FundamentalSource::Fixed => base,
        FundamentalSource::Tracked => base + 2.0 * COLUMN_GAP + tracking + target,
    }
}

impl EquartEditor {
    pub(super) fn harmonic_modifier(&self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let region = &self.params.regions[index];
        let harmonic = &region.harmonic;
        let bypassed = harmonic.bypassed.value();
        let accent = regions::REGION_COLOR;
        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let (toggle, remove) = modifier_header(ui, header, index, "HARMONIC", !bypassed, accent);

        if toggle {
            self.set(&setter, &harmonic.bypassed, !bypassed);
        }

        if remove {
            self.set(&setter, &harmonic.enabled, false);
            return;
        }

        let mut inner = rect.shrink2(Vec2::new(HORIZONTAL_INSET, 7.0));
        inner.min.x += HEADER_WIDTH;
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("harmonic-modifier", index))
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                let [mask_width, source_width, tracking_width, target_width] = COLUMNS;
                let detection = &harmonic.detection;
                let source = harmonic.source.value();

                column(ui, mask_width, |ui| {
                    ui.strong("Mask");
                    ui.horizontal(|ui| {
                        for (mode, name, help) in [
                            (
                                HarmonicMode::Only,
                                "Only",
                                "Apply the region gain to the selected harmonics.",
                            ),
                            (
                                HarmonicMode::Protect,
                                "Protect",
                                "Preserve the selected harmonics and cut the rest of the region.",
                            ),
                        ] {
                            if ui
                                .selectable_label(harmonic.mode.value() == mode, name)
                                .on_hover_text(help)
                                .clicked()
                            {
                                self.set(&setter, &harmonic.mode, mode);
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let number_size = Vec2::new(24.0, ui.spacing().interact_size.y);
                        ui.label("Harmonics");
                        let number = |ui: &mut egui::Ui, param: &IntParam, label| {
                            let mut value = param.value();
                            if ui
                                .add(
                                    egui::DragValue::new(&mut value)
                                        .min_size(number_size)
                                        .range(1..=MAX_HARMONIC as i32),
                                )
                                .on_hover_text(label)
                                .changed()
                            {
                                self.set(&setter, param, value);
                            }
                        };
                        number(ui, &harmonic.first, "First harmonic");
                        ui.label("–");
                        number(ui, &harmonic.last, "Last harmonic");
                    });
                    self.dynamic_number(ui, &setter, "Width", &harmonic.width, 0.5)
                        .on_hover_text(
                            "Bandwidth as a percentage of the spacing between harmonics.",
                        );
                });

                column(ui, source_width, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui.strong("Fundamental");
                    self.pitch_choice(
                        ui,
                        &setter,
                        &harmonic.source,
                        &[FundamentalSource::Tracked, FundamentalSource::Fixed],
                        FundamentalSource::label,
                    );
                    match source {
                        FundamentalSource::Tracked => {
                            self.pitch_detection_choices(ui, &setter, detection)
                        }
                        FundamentalSource::Fixed => {
                            self.dynamic_number(ui, &setter, "Freq", &harmonic.fixed_hz, 1.0);
                        }
                    }

                    if bypassed || region.bypass.value() || self.params.global.bypass.value() {
                        ui.label("Bypassed");
                    } else if region.gain.value() == 0.0 {
                        ui.label("Gain is 0 dB");
                    } else if source == FundamentalSource::Tracked {
                        if let Some(hz) = self.telemetry.harmonic_hz(index) {
                            ui.colored_label(accent, format!("{hz:.1} Hz"));
                        } else {
                            ui.label("Waiting for pitch");
                        }
                    }
                });

                if source == FundamentalSource::Tracked {
                    column(ui, tracking_width, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.pitch_tracking_controls(
                            ui,
                            &setter,
                            detection,
                            "Smooths changes in the detected fundamental.",
                        );
                    });
                    column(ui, target_width, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        self.pitch_target_controls(ui, &setter, detection)
                    });
                }
            },
        );
    }
}
