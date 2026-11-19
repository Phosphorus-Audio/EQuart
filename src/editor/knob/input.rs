use egui::{Color32, Response, Ui};
use nice_plug::{context::gui::ParamSetter, prelude::*};

#[derive(Clone)]
struct NumericEntry {
    binding: usize,
    text: String,
    error: Option<String>,
    focus: bool,
}

// just a wrap, dont ask
pub(super) fn value_text(param: &FloatParam, normalized: f32) -> String {
    param.normalized_value_to_string(normalized, true)
}

fn parse_value(param: &FloatParam, text: &str) -> Result<f32, String> {
    let cleaned = text.trim().replace(',', ".");
    let token = cleaned.split_whitespace().next().unwrap_or("");
    let (number, factor) = match token.strip_suffix(['k', 'K']) {
        Some(number) => (number, 1_000.0),
        None => (token, 1.0),
    };

    let value = number
        .parse::<f32>()
        .map_err(|_| "Enter a valid number".to_owned())?
        * factor;

    if !value.is_finite() {
        return Err("Enter a finite number".to_owned());
    }

    let a = param.preview_plain(0.0);
    let b = param.preview_plain(1.0);
    let (min, max) = (a.min(b), a.max(b));

    if !(min..=max).contains(&value) {
        return Err(format!("Range: {min:.2} to {max:.2}"));
    }

    Ok(value)
}

/// right-click here
pub(super) fn edit_value(
    ui: &mut Ui,
    response: &Response,
    param: &FloatParam,
    setter: &ParamSetter<'_>,
    label: &str,
    read_only: bool,
) -> bool {
    let state_id = response.id.with("numeric-entry");
    let popup_id = egui::Popup::default_response_id(response);
    let binding = param as *const FloatParam as usize;
    let mut entry = ui.data(|data| data.get_temp::<NumericEntry>(state_id));

    if read_only {
        if entry.is_some() {
            egui::Popup::close_id(ui.ctx(), popup_id);
            ui.data_mut(|data| data.remove::<NumericEntry>(state_id));
        }
        return false;
    }

    if response.secondary_clicked() {
        entry = Some(NumericEntry {
            binding,
            text: param.normalized_value_to_string(param.unmodulated_normalized_value(), false),
            error: None,
            focus: true,
        });
    }

    let Some(mut entry) = entry else { return false };

    if entry.binding != binding {
        egui::Popup::close_id(ui.ctx(), popup_id);
        ui.data_mut(|data| data.remove::<NumericEntry>(state_id));
        return false;
    }

    let mut committed = false;

    egui::Popup::context_menu(response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .frame(egui::Frame::popup(ui.style()).fill(Color32::from_gray(38)))
        .width(200.0)
        .show(|ui| {
            ui.label(label);

            let mut edit = egui::TextEdit::singleline(&mut entry.text)
                .id(state_id.with("text"))
                .desired_width(180.0)
                .show(ui);

            if entry.focus && ui.is_visible() {
                edit.response.request_focus();
                edit.state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::select_all_str(&entry.text)));
                edit.state.store(ui.ctx(), edit.response.id);
                entry.focus = false;
            }

            let unit = param.unit().trim();
            if !unit.is_empty() {
                ui.label(unit);
            }

            if let Some(error) = &entry.error {
                ui.colored_label(Color32::from_rgb(230, 92, 92), error);
            }

            let enter = edit.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut apply = enter;
            let mut cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));

            ui.horizontal(|ui| {
                apply |= ui.button("Apply").clicked();
                cancel |= ui.button("Cancel").clicked();
            });

            if cancel {
                egui::Popup::close_id(ui.ctx(), popup_id);
            } else if apply {
                match parse_value(param, &entry.text) {
                    Ok(value) => {
                        setter.begin_set_parameter(param);
                        setter.set_parameter(param, value);
                        setter.end_set_parameter(param);
                        committed = true;
                        egui::Popup::close_id(ui.ctx(), popup_id);
                    }
                    Err(error) => entry.error = Some(error),
                }
            }
        });

    if egui::Popup::is_id_open(ui.ctx(), popup_id) {
        ui.data_mut(|data| data.insert_temp(state_id, entry));
    } else {
        ui.data_mut(|data| data.remove::<NumericEntry>(state_id));
    }

    committed
}
