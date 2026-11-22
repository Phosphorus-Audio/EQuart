use super::*;
// why not use crate?

#[derive(Clone)]
struct Entry {
    key: u32,
    rect: Rect,
    options: Vec<String>,
    current: usize,
    progress: f32,
    open: bool,
}

pub(crate) struct Dropdown {
    group: egui::Id,
    open: u32,
    entries: Vec<Entry>,
    combos: usize,
    stepped: Option<(u32, usize)>,
}

impl Dropdown {
    pub(crate) fn new(ctx: &egui::Context, group: egui::Id) -> Self {
        let open = ctx.data(|d| d.get_temp::<u32>(group.with("open")).unwrap_or(0));
        Self {
            group,
            open,
            entries: Vec::new(),
            combos: 0,
            stepped: None,
        }
    }

    pub(crate) fn new_group(&mut self) {
        self.combos = 0;
    }

    /// A caption above a value button
    /// FULL COMBO!
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn combo(
        &mut self,
        ui: &mut egui::Ui,
        key: u32,
        label: &str,
        value: &str,
        options: &[&str],
        current: usize,
        accent: Color32,
    ) {
        if self.combos > 0 {
            ui.add_space(COMBO_LEAD);
        }
        self.combos += 1;

        let (caption, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), COMBO_CAPTION_H),
            Sense::hover(),
        );
        let heading = label.to_uppercase();
        let anchor = caption.left_center() + Vec2::new(1.0, 0.0);
        let font = FontId::proportional(9.5);
        label_plate(
            ui,
            &heading,
            font.clone(),
            anchor,
            egui::Align2::LEFT_CENTER,
        );
        ui.painter().text(
            anchor,
            egui::Align2::LEFT_CENTER,
            heading,
            font,
            theme::current(ui.ctx()).text.heading,
        );

        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), COMBO_BUTTON_H),
            Sense::click(),
        );
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            self.open = if self.open == key { 0 } else { key };
        }
        let open = self.open == key;
        if let Some(index) = wheel_index(
            &response,
            self.open == 0 && !response.clicked(),
            Some(current),
            options.len(),
        ) {
            self.stepped = Some((key, index));
        }

        paint_combo_button(ui, &response, value, open, true, accent, true);

        let progress = ui.ctx().animate_bool_with_time_and_easing(
            self.group.with(key),
            open,
            0.18,
            egui::emath::easing::cubic_out,
        );
        self.entries.push(Entry {
            key,
            rect,
            options: options.iter().map(|s| (*s).to_owned()).collect(),
            current,
            progress,
            open,
        });
    }

    pub(crate) fn finish(
        mut self,
        ui: &mut egui::Ui,
        bounds: Rect,
        accent: Color32,
    ) -> Option<(u32, usize)> {
        let Some(entry) = self
            .entries
            .iter()
            .max_by(|a, b| a.progress.total_cmp(&b.progress))
            .filter(|e| e.progress > 0.001)
        else {
            self.persist(ui.ctx());
            return self.stepped;
        };

        let Entry {
            key,
            rect,
            options,
            current,
            progress,
            open,
        } = entry.clone();

        let t = progress;
        let p = ui.painter();
        let panel = theme::current(ui.ctx()).panel;
        let scrim = ui.interact(bounds, self.group.with("scrim"), Sense::click());

        p.rect_filled(bounds, 8.0, Color32::from_black_alpha((78.0 * t) as u8));

        let count = options.len();
        let height = count as f32 * OPTION_H + 10.0;
        let width = rect.width().max(110.0).min(bounds.width() - 12.0);
        let list = option_list_rect(rect, Vec2::new(width, height), bounds);

        let scale = 0.94 + 0.06 * egui::emath::easing::back_out(t);
        let plate = Rect::from_center_size(list.center(), list.size() * scale)
            .translate(Vec2::new(0.0, (1.0 - t) * -7.0));
        for layer in (0..10).rev() {
            let grow = 1.0 + 8.0 * layer as f32 / 9.0;
            p.rect_filled(
                plate.expand2(Vec2::new(grow, grow + 2.0)),
                9.0,
                Color32::from_black_alpha((7.0 * t * (1.0 - layer as f32 / 10.0)) as u8),
            );
        }
        p.rect_filled(plate, 8.0, fade(panel.option, t));
        p.rect_stroke(
            plate,
            8.0,
            Stroke::new(1.0, fade(panel.option_border, t)),
            egui::StrokeKind::Inside,
        );

        let mut picked = None;
        for (index, option) in options.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(
                    list.left() + 5.0,
                    list.top() + 5.0 + index as f32 * OPTION_H,
                ),
                Vec2::new(width - 10.0, OPTION_H - 1.0),
            );
            let response = ui.interact(row, self.group.with(key).with(index), Sense::click());

            let row_t = ((t * (count as f32 + 3.0) - index as f32) / 3.0).clamp(0.0, 1.0);
            let shown = row.translate(Vec2::new(0.0, (1.0 - row_t) * 7.0));
            paint_option(
                p,
                shown,
                option,
                index == current,
                hover_amount(&response),
                accent,
                row_t,
                None,
            );
            if open && response.clicked() {
                picked = Some((key, index));
            }
        }

        if picked.is_some() || scrim.clicked() {
            self.open = 0;
        }
        self.persist(ui.ctx());
        picked
    }

    fn persist(&self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(self.group.with("open"), self.open));
    }
}

pub(super) fn wheel_index(
    response: &Response,
    closed: bool,
    current: Option<usize>,
    count: usize,
) -> Option<usize> {
    use crate::editor::wheel::{take_steps, wheel_steps};

    let ctx = &response.ctx;
    let id = response.id.with("wheel-travel");
    let Some(current) = current.filter(|&index| index < count && count > 1) else {
        ctx.data_mut(|data| data.remove::<(u64, f32)>(id));
        return None;
    };

    let modified =
        ctx.input(|input| input.modifiers.shift || input.modifiers.ctrl || input.modifiers.command);
    if !closed || !response.hovered() || !response.enabled() || modified {
        ctx.data_mut(|data| data.remove::<(u64, f32)>(id));
        return None;
    }

    let frame = ctx.cumulative_frame_nr();
    let previous = ctx.data(|data| data.get_temp::<(u64, f32)>(id));
    if previous.is_some_and(|(last, _)| last == frame) {
        return None;
    }

    let travel = ctx.input_mut(|input| {
        // use physical wheel events so egui's smoothing cannot repeat a step
        let travel = wheel_steps(&input.raw.events);
        input.smooth_scroll_delta.y = 0.0;
        travel
    });

    let remainder = previous
        .filter(|(last, _)| last + 1 == frame)
        .map_or(0.0, |(_, rest)| rest);
    let (steps, remainder) = take_steps(remainder - travel);
    let next = current.saturating_add_signed(steps as isize).min(count - 1);
    let at_end = (next == 0 && remainder < 0.0) || (next + 1 == count && remainder > 0.0);

    ctx.data_mut(|data| data.insert_temp(id, (frame, if at_end { 0.0 } else { remainder })));

    (next != current).then_some(next)
}

pub(super) fn option_list_rect(anchor: Rect, size: Vec2, bounds: Rect) -> Rect {
    let x = anchor
        .left()
        .clamp(bounds.left() + 6.0, bounds.right() - 6.0 - size.x);
    let below = anchor.bottom() + 4.0;
    let y = if below + size.y <= bounds.bottom() - 6.0 {
        below
    } else {
        (anchor.top() - 4.0 - size.y).max(bounds.top() + 6.0)
    }
    .min(bounds.bottom() - 6.0 - size.y);
    Rect::from_min_size(Pos2::new(x, y), size)
}
