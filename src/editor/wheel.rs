const WHEEL_POINTS_PER_STEP: f32 = 40.0;

pub(super) fn wheel_steps(events: &[egui::Event]) -> f32 {
    events
        .iter()
        .map(|event| match event {
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                ..
            } => delta.y / WHEEL_POINTS_PER_STEP,
            egui::Event::MouseWheel { delta, .. } => delta.y,
            _ => 0.0,
        })
        .sum()
}

pub(super) fn take_steps(travel: f32) -> (i32, f32) {
    let steps = travel.trunc() as i32;
    (steps, travel - steps as f32)
}
