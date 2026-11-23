use super::*;

#[derive(Clone, Copy)]
struct TabMotion {
    from: f32,
    selected: usize,
    started: f64,
}

impl TabMotion {
    fn progress(self, now: f64) -> f32 {
        egui::emath::easing::cubic_out(((now - self.started) as f32 / 0.24).clamp(0.0, 1.0))
    }

    fn position(self, now: f64) -> f32 {
        egui::lerp(self.from..=self.selected as f32, self.progress(now))
    }
}

pub(crate) fn segmented_tabs(
    ui: &mut egui::Ui,
    id: egui::Id,
    labels: [&str; 2],
    selected: usize,
    accent: Color32,
) -> (usize, f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    let palette = theme::current(ui.ctx());
    let p = ui.painter();

    p.rect_filled(rect, 7.0, palette.dropdown.background.gamma_multiply(0.85));
    p.rect_stroke(
        rect,
        7.0,
        Stroke::new(1.0, palette.panel.border.gamma_multiply(0.45)),
        egui::StrokeKind::Inside,
    );

    let pad = 2.0;
    let slot_w = (rect.width() - pad * 2.0) * 0.5;
    let cells = std::array::from_fn::<_, 2, _>(|index| {
        Rect::from_min_size(
            Pos2::new(rect.left() + pad + index as f32 * slot_w, rect.top() + pad),
            Vec2::new(slot_w, rect.height() - pad * 2.0),
        )
    });
    let responses = std::array::from_fn::<_, 2, _>(|index| {
        ui.interact(cells[index], id.with(index), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    });
    let next = responses
        .iter()
        .position(Response::clicked)
        .unwrap_or(selected);

    let now = ui.input(|input| input.time);
    let motion_id = id.with("motion");
    let motion = ui.data_mut(|data| {
        let motion = data.get_temp_mut_or_insert_with(motion_id, || TabMotion {
            from: selected as f32,
            selected,
            started: f64::NEG_INFINITY,
        });

        if motion.selected != next {
            *motion = TabMotion {
                from: motion.position(now),
                selected: next,
                started: now,
            };
        }

        *motion
    });
    let progress = motion.progress(now);
    let slide = motion.position(now);

    if progress < 1.0 {
        ui.ctx().request_repaint();
    }

    let stretch = 4.0 * slide * (1.0 - slide);
    let pill = cells[0]
        .translate(Vec2::new(slide * slot_w, 0.0))
        .expand2(Vec2::new(2.8 * stretch, -0.7 * stretch));
    if stretch > 0.0 {
        p.rect_filled(pill.expand(1.0), 6.5, fade(accent, 0.16 * stretch));
    }

    p.rect_filled(pill, 5.5, palette.dropdown.selected);
    p.rect_stroke(
        pill,
        5.5,
        Stroke::new(1.0, accent.gamma_multiply(0.55)),
        egui::StrokeKind::Inside,
    );

    for (index, ((label, cell), response)) in
        labels.into_iter().zip(cells).zip(&responses).enumerate()
    {
        let hover = hover_amount(response);
        let selected = 1.0 - (slide - index as f32).abs();
        let color = palette.text.muted.lerp_to_gamma(
            palette.dropdown.text,
            egui::lerp((hover * 0.65)..=1.0, selected),
        );
        let font = FontId::proportional(11.0);

        label_plate(
            ui,
            label,
            font.clone(),
            cell.center(),
            egui::Align2::CENTER_CENTER,
        );
        p.text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            color,
        );
    }

    (next, progress)
}

pub(crate) fn slider(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    value: &mut u8,
    default: u8,
    accent: Color32,
) -> bool {
    let width = ui.available_width();
    let palette = theme::current(ui.ctx());

    let (header, _) = ui.allocate_exact_size(Vec2::new(width, COMBO_CAPTION_H), Sense::hover());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    let response = ui
        .interact(rect, id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let pressing =
        response.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_down());
    let track_left = rect.left() + 3.0;
    let track_right = (rect.right() - 3.0).max(track_left + 1.0);
    let span = track_right - track_left;
    let previous = *value;

    #[cfg(target_os = "windows")]
    if pressing {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    }

    if response.double_clicked() {
        *value = default;
    } else if (response.clicked() || response.dragged_by(egui::PointerButton::Primary))
        && let Some(pos) = response.interact_pointer_pos()
    {
        let fraction = ((pos.x - track_left) / span).clamp(0.0, 1.0);
        *value = (fraction * 100.0).round() as u8;
    } else if response.hovered() {
        let delta = ui.input(|input| input.smooth_scroll_delta.y);

        if delta != 0.0 {
            let step = if delta > 0.0 { 2 } else { -2 };
            *value = (i32::from(*value) + step).clamp(0, 100) as u8;
        }
    }

    let heading = label.to_uppercase();
    let heading_pos = header.left_center() + Vec2::new(1.0, 0.0);
    let heading_font = FontId::proportional(9.5);
    let readout = format!("{value}%");
    let readout_pos = header.right_center() - Vec2::new(1.0, 0.0);
    let readout_font = FontId::proportional(10.5);

    label_plate(
        ui,
        &heading,
        heading_font.clone(),
        heading_pos,
        egui::Align2::LEFT_CENTER,
    );
    ui.painter().text(
        heading_pos,
        egui::Align2::LEFT_CENTER,
        heading,
        heading_font,
        palette.text.heading,
    );
    ui.painter().text(
        readout_pos,
        egui::Align2::RIGHT_CENTER,
        readout,
        readout_font,
        palette.text.value,
    );

    let p = ui.painter();
    let hover = hover_amount(&response);
    let press = ui.ctx().animate_bool_with_time_and_easing(
        id.with("press"),
        pressing,
        0.15,
        egui::emath::easing::quadratic_out,
    );

    let active = hover * (1.0 - press);
    let half_height = 2.0 + 0.9 * active + 2.2 * press;
    let center_y = rect.center().y;
    let bar = |right| {
        Rect::from_min_max(
            Pos2::new(track_left, center_y - half_height),
            Pos2::new(right, center_y + half_height),
        )
    };

    let track = bar(track_right);
    let radius = half_height.round();
    let track_color = palette
        .switch
        .colors
        .track_off
        .lerp_to_gamma(palette.switch.colors.track_on, 0.17 * active + 0.32 * press);
    let fraction = f32::from((*value).min(100)) / 100.0;
    let fill = bar(track_left + fraction * span);
    let lit = accent.lerp_to_gamma(palette.switch.colors.thumb, 0.20 * active + 0.55 * press);

    let glow = 0.10 * active + 0.36 * press;
    if glow > 0.0 {
        let source = if fraction > 0.0 { fill } else { track };

        for (spread, strength) in [(4.0, 0.16), (2.5, 0.28), (1.0, 0.56)] {
            p.rect_filled(
                source.expand(spread),
                radius + spread,
                fade(lit, glow * strength),
            );
        }
    }

    p.rect_filled(track, radius, track_color);
    if fraction > 0.0 {
        p.rect_filled(fill, radius.min(fill.width() * 0.5), lit);
    }

    *value != previous
}
