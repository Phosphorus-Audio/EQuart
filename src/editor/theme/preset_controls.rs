use super::*;

pub(in crate::editor) enum PresetDialog {
    Save { name: String, focus_name: bool },
    Delete(String),
}

impl EquartEditor {
    pub(super) fn theme_preset_row(&mut self, ui: &mut egui::Ui, draft: &mut Theme) -> bool {
        let saved = self.display_preferences.theme_presets();
        let selected = saved
            .iter()
            .map(|(name, theme)| (name.as_str(), theme))
            .chain(
                presets::builtins()
                    .iter()
                    .map(|(name, theme)| (*name, theme)),
            )
            .find(|(_, theme)| **theme == *draft)
            .map_or("Custom", |(name, _)| name);
        let mut loaded = false;

        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("theme-presets")
                .selected_text(selected)
                .width(180.0)
                .truncate()
                .show_ui(ui, |ui| {
                    for (name, theme) in presets::builtins() {
                        if ui.selectable_label(selected == *name, *name).clicked() {
                            *draft = *theme;
                            loaded = true;
                            ui.close();
                        }
                    }

                    if !saved.is_empty() {
                        ui.separator();
                    }

                    for (name, theme) in &saved {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(selected == name, name).clicked() {
                                *draft = *theme;
                                loaded = true;
                                ui.close();
                            }

                            if preset_icon(ui, icons::Glyph::DeletePreset, "Delete preset")
                                .clicked()
                            {
                                self.theme_preset_dialog = Some(PresetDialog::Delete(name.clone()));
                                ui.close();
                            }
                        });
                    }
                });

            if preset_icon(ui, icons::Glyph::SavePreset, "Save theme preset").clicked() {
                let name = if saved.contains_key(selected) {
                    selected.to_owned()
                } else {
                    String::new()
                };

                self.theme_preset_dialog = Some(PresetDialog::Save {
                    name,
                    focus_name: true,
                });
            }

            if ui
                .button("Folder")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Open theme folder")
                .clicked()
                && let Err(error) = self.display_preferences.reveal_theme_folder()
            {
                self.notify(
                    format!("Could not open the theme folder: {error}"),
                    ui.input(|input| input.time),
                );
            }
        });

        loaded
    }

    pub(super) fn show_theme_preset_dialog(&mut self, ctx: &egui::Context, palette: Theme) {
        let Some(dialog) = &mut self.theme_preset_dialog else {
            return;
        };
        let saved = self.display_preferences.theme_presets();
        let modal =
            egui::Modal::new(egui::Id::unique("equart-theme-preset-dialog")).show(ctx, |ui| {
                let (label, enabled) = match dialog {
                    PresetDialog::Save { name, focus_name } => {
                        ui.heading("Save theme preset");
                        ui.label("Preset name");
                        let input = ui.text_edit_singleline(name);

                        if std::mem::take(focus_name) {
                            input.request_focus();
                        }

                        let name = name.trim();
                        let label = if saved.contains_key(name) {
                            "Update preset"
                        } else {
                            "Save preset"
                        };
                        let valid =
                            crate::presets::is_valid_name(name) && !presets::is_builtin_name(name);

                        (label, valid)
                    }
                    PresetDialog::Delete(name) => {
                        ui.heading("Delete theme preset?");
                        ui.label(name.as_str());
                        ("Delete preset", true)
                    }
                };

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        ui.close();
                    }

                    ui.add_enabled(enabled, egui::Button::new(label))
                        .on_disabled_hover_text("Enter a custom preset name")
                        .clicked()
                })
                .inner
            });

        if modal.inner {
            let result = match dialog {
                PresetDialog::Save { name, .. } => self.display_preferences.save_theme_preset(
                    name,
                    palette,
                    saved.contains_key(name.trim()),
                ),
                PresetDialog::Delete(name) => self.display_preferences.delete_theme_preset(name),
            };

            match result {
                Ok(()) => self.theme_preset_dialog = None,
                Err(error) => {
                    nice_plug::nice_error!("Could not update EQuart theme presets: {error}");
                    self.notify(
                        format!("Could not update theme presets: {error}"),
                        ctx.input(|input| input.time),
                    );
                }
            }
            ctx.request_repaint();
        } else if modal.should_close() {
            self.theme_preset_dialog = None;
            ctx.request_repaint();
        }
    }
}

fn preset_icon(ui: &mut egui::Ui, glyph: icons::Glyph, tooltip: &str) -> Response {
    let texture = icons::textures(ui.ctx()).id(glyph);
    let icon = egui::Image::new((texture, Vec2::splat(14.0))).tint(ui.visuals().text_color());

    ui.add(egui::Button::image(icon))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}
