use super::*;
use crate::display_preferences::theme::{ColorMode, SpectrumPaint, presets};

mod preset_controls;

pub(super) use preset_controls::PresetDialog;

const FRAME_THEME: &str = "equart-frame-theme";

pub(super) fn current(ctx: &egui::Context) -> Theme {
    ctx.data(|data| data.get_temp(egui::Id::unique(FRAME_THEME)))
        .unwrap_or_default()
}

pub(super) fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub(super) fn install(ctx: &egui::Context, theme: Theme, accent: Color32) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(FRAME_THEME), theme));
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = theme.background.bottom;
        visuals.window_fill = theme.panel.menu;
        visuals.window_stroke.color = theme.panel.border;
        visuals.override_text_color = Some(theme.text.primary);
        visuals.weak_text_color = Some(theme.text.muted);
        visuals.text_edit_bg_color = Some(theme.panel.option);
        visuals.selection.bg_fill = accent.gamma_multiply(0.3);
        visuals.selection.stroke = Stroke::new(1.0, accent);
        visuals.widgets.inactive.weak_bg_fill = theme.panel.option;
        visuals.widgets.hovered.weak_bg_fill = theme.panel.hover;
        visuals.widgets.active.weak_bg_fill = theme.panel.selected;
    });
}

pub(super) fn scroll<R>(
    ui: &mut egui::Ui,
    area: egui::ScrollArea,
    colors: crate::display_preferences::theme::Scrollbar,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let style = Arc::clone(ui.style());

    ui.scope(|ui| {
        ui.spacing_mut().scroll.foreground_color = false;
        let visuals = ui.visuals_mut();
        visuals.extreme_bg_color = colors.track;
        visuals.widgets.inactive.bg_fill = colors.thumb;
        visuals.widgets.hovered.bg_fill = colors.hover;
        visuals.widgets.active.bg_fill = colors.active;

        area.show(ui, |content| {
            content.set_style(style);
            body(content)
        })
        .inner
    })
    .inner
}

impl EquartEditor {
    pub(super) fn theme_for_frame(&mut self, ctx: &egui::Context) -> Theme {
        if let Some((theme, deadline)) = self.pending_theme {
            let now = ctx.input(|input| input.time);

            if self.theme_open && now < deadline {
                ctx.request_repaint_after(Duration::from_secs_f64(deadline - now));
                return theme;
            }

            self.flush_theme(Some(ctx));
        }

        self.display_preferences.read().theme
    }

    pub(super) fn flush_theme(&mut self, ctx: Option<&egui::Context>) {
        let Some((theme, _)) = self.pending_theme.take() else {
            return;
        };

        if let Err(error) = self.display_preferences.update(|prefs| prefs.theme = theme) {
            nice_plug::nice_error!("Could not save EQuart theme: {error}");

            if let Some(ctx) = ctx {
                self.notify(
                    format!("Could not save theme: {error}"),
                    ctx.input(|input| input.time),
                );
            }
        }
    }

    pub(super) fn theme_popup(&mut self, button: &Response) {
        let ctx = &button.ctx;

        if button.clicked() {
            self.theme_open = !self.theme_open;
            ctx.request_repaint();
        }

        if !self.theme_open {
            self.flush_theme(Some(ctx));
            self.theme_preset_dialog = None;
            return;
        }

        let mut draft = self.theme;
        let mut selected_preset = false;
        let mut close = false;
        let modal_open = self.theme_preset_dialog.is_some();
        let max_height = (ctx.viewport_rect().height() - 140.0).min(425.0);

        let popup = egui::Popup::from_response(button)
            .open(true)
            .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
            .align(egui::emath::RectAlign::TOP_END)
            .gap(5.0)
            .width(300.0)
            .show(|ui| {
                ui.set_width(300.0);
                ui.heading("Theme");
                ui.add_space(4.0);
                selected_preset = self.theme_preset_row(ui, &mut draft);
                ui.separator();

                scroll(
                    ui,
                    egui::ScrollArea::vertical()
                        .max_height(max_height)
                        .auto_shrink([false, false]),
                    draft.scrollbar,
                    |ui| edit_theme(ui, &mut draft),
                );
                ui.separator();
                close = ui.button("Close").clicked();
            });

        if draft != self.theme {
            let deadline = ctx.input(|input| input.time) + 0.2;
            self.pending_theme = Some((draft, deadline));
            ctx.request_repaint();
        }

        let dismissed = popup.is_some_and(|popup| popup.response.should_close());
        self.theme_open = !close && (!dismissed || modal_open);

        if selected_preset || !self.theme_open {
            self.flush_theme(Some(ctx));
        }

        if self.theme_open {
            self.show_theme_preset_dialog(ctx, draft);
        } else {
            self.theme_preset_dialog = None;
            ctx.request_repaint();
        }
    }
}

fn edit_theme(ui: &mut egui::Ui, theme: &mut Theme) {
    egui::CollapsingHeader::new("Spectrum fill")
        .default_open(true)
        .show(ui, |ui| {
            spectrum_paint(ui, &mut theme.spectrum_fill);
        });
    egui::CollapsingHeader::new("Spectrum outline").show(ui, |ui| {
        spectrum_paint(ui, &mut theme.spectrum_trace);
    });

    for (title, palette) in [
        ("Band palette", theme.band.as_mut_slice()),
        ("Spectral regions", theme.region.as_mut_slice()),
    ] {
        egui::CollapsingHeader::new(title).show(ui, |ui| {
            for (index, chunk) in palette.chunks_mut(8).enumerate() {
                ui.horizontal(|ui| {
                    for (column, color) in chunk.iter_mut().enumerate() {
                        let slot = index * 8 + column + 1;
                        ui.push_id(slot, |ui| {
                            ui.vertical(|ui| {
                                ui.label(slot.to_string());
                                color_button(ui, color);
                            });
                        });
                    }
                });
            }
        });
    }

    macro_rules! section {
        ($label:literal, $colors:expr) => {
            egui::CollapsingHeader::new($label).show(ui, |ui| {
                $colors.edit(|label, color| edit_color(ui, label, color));
            });
        };
    }

    section!("Background", theme.background);
    section!("Grid", theme.grid);
    section!("Text", theme.text);
    section!("Panels and controls", theme.panel);
    section!("EQ and phase curves", theme.curves);
    section!("Level meters", theme.meter);
    section!("Scrollbars", theme.scrollbar);
    section!("Piano keys", theme.piano);
    egui::CollapsingHeader::new("Knobs").show(ui, |ui| {
        ui.checkbox(
            &mut theme.knob.follow_accent,
            "Use the current accent for the value arc",
        );
        theme
            .knob
            .colors
            .edit(|label, color| edit_color(ui, label, color));
    });
}

fn spectrum_paint(ui: &mut egui::Ui, paint: &mut SpectrumPaint) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut paint.mode, ColorMode::Solid, "Solid");
        ui.selectable_value(&mut paint.mode, ColorMode::Gradient, "Gradient");
    });

    match paint.mode {
        ColorMode::Solid => edit_color(ui, "Color", &mut paint.solid),
        ColorMode::Gradient => {
            for (label, color) in ["Sub", "Bass", "Mid", "High", "Air"]
                .into_iter()
                .zip(&mut paint.stops)
            {
                edit_color(ui, label, color);
            }
        }
    }
}

fn edit_color(ui: &mut egui::Ui, label: &str, color: &mut Color32) {
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            color_button(ui, color);
            ui.label(label);
        });
    });
}

fn color_button(ui: &mut egui::Ui, color: &mut Color32) {
    let mut edited = *color;

    if egui::color_picker::color_edit_button_srgba(
        ui,
        &mut edited,
        egui::color_picker::Alpha::Opaque,
    )
    .changed()
    {
        *color = edited;
    }
}
