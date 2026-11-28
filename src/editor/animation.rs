use super::*;

const SPEED_ID: &str = "equart-animation-speed";

pub(super) fn speed(ctx: &egui::Context) -> AnimationSpeed {
    ctx.data(|data| data.get_temp(egui::Id::unique(SPEED_ID)))
        .unwrap_or_default()
}

pub(super) fn install(ui: &mut egui::Ui, selected: AnimationSpeed) {
    if speed(ui.ctx()) == selected {
        return;
    }

    let ctx = ui.ctx();
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(SPEED_ID), selected));

    let seconds = selected.duration(egui::Style::default().animation_time);
    ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = seconds);
    ui.style_mut().animation_time = seconds;
}
