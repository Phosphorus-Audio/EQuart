use super::*;

const BAND_PANEL_RADIUS: u8 = 10;
pub(in crate::editor) const BAND_ACTION_WIDTH: f32 = 36.0;
const BAND_ACTION_GLYPH: f32 = 14.0;


fn caret_icon(p: &egui::Painter, center: Pos2, color: Color32, size: f32, rightwards: bool) {
    let unit = size / 24.0;
    let at = |x: f32, y: f32| center + Vec2::new(x - 12.0, y - 12.0) * unit;

    let points = if rightwards {
        vec![at(8.0, 6.4), at(15.9, 12.0), at(8.0, 17.6)]
    } else {
        vec![at(16.0, 6.4), at(8.1, 12.0), at(16.0, 17.6)]
    };

    p.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}


pub(in crate::editor) fn power_icon(p: &egui::Painter, center: Pos2, color: Color32, size: f32) {
    let unit = size / 24.0;
    let at = |x: f32, y: f32| center + Vec2::new(x - 12.0, y - 12.0) * unit;

    p.rect_filled(
        Rect::from_min_max(at(11.0, 2.0), at(13.0, 12.0)),
        0.5 * unit,
        color,
    );

    let start = 315.0_f32.to_radians();
    let points: Vec<Pos2> = (0..=72)
        .map(|i| {
            let angle = start + 1.5 * std::f32::consts::PI * i as f32 / 72.0;
            center + Vec2::angled(angle) * (8.0 * unit)
        })
        .collect();
    p.add(egui::Shape::line(points, Stroke::new(2.0 * unit, color)));
}


pub(in crate::editor) fn soft_shadow(
    p: &egui::Painter,
    rect: Rect,
    radius: f32,
    spread: f32,
    drop: f32,
) {
    const LAYERS: usize = 30;


    for layer in (0..LAYERS).rev() {
        let t = layer as f32 / (LAYERS - 1) as f32;
        let grow = spread * t;
        let alpha = (1.8 + 6.0 * (1.0 - t).powi(2)) as u8;

        p.rect_filled(
            rect.translate(Vec2::new(0.0, drop)).expand(grow),
            radius + grow,
            Color32::from_black_alpha(alpha),
        );
    }
}


pub(in crate::editor) fn panel_plate(p: &egui::Painter, panel: Rect) {
    soft_shadow(p, panel, f32::from(BAND_PANEL_RADIUS), 58.0, 14.0);
    p.rect_filled(
        panel,
        BAND_PANEL_RADIUS,
        theme::with_alpha(theme::current(p.ctx()).panel.parameter, 242),
    );
}

pub(in crate::editor) fn panel_actions(
    ui: &mut egui::Ui,
    panel: Rect,
    target: modifiers::ModifierTarget,
    add_available: bool,
    can_convert: bool,
) -> [Response; 4] {
    use icons::Glyph;

    let (convert, switch, full) = match target {
        modifiers::ModifierTarget::Band(_) => (
            Glyph::ToRegion,
            "Switch to Spectral Region",
            "No free Spectral Region slots",
        ),
        modifiers::ModifierTarget::Region(_) => (
            Glyph::ToBand,
            "Switch to Band",
            "No free Band slots",
        ),
    };
    let height = panel.height() / 4.0;
    let buttons = [
        (Glyph::Close, "", true),
        (Glyph::Listen, "Hold to listen", true),
        (
            convert,
            if can_convert { switch } else { full },
            can_convert,
        ),
        (Glyph::Add, "Add modifier", true),
    ];
    std::array::from_fn(|row| {
        let (glyph, label, enabled) = buttons[row];
        let rect = Rect::from_min_size(
            Pos2::new(
                panel.right() - BAND_ACTION_WIDTH,
                panel.top() + height * row as f32,
            ),
            Vec2::new(BAND_ACTION_WIDTH, height),
        );
        let response = ui
            .interact(
                rect,
                ui.make_persistent_id(("panel-action", target, glyph as u8)),
                if glyph == Glyph::Listen {
                    Sense::click_and_drag()
                } else if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            )
            .on_hover_cursor(if enabled {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Default
            });
        let response = if glyph == Glyph::Close {
            close_tooltip(ui, &response);
            response
        } else {
            response.on_hover_text(label)
        };
        let p = ui.painter();
        let hover = if enabled {
            hover_amount(&response)
        } else {
            0.0
        };
        if hover > 0.0 {
            let rounding = egui::CornerRadius {
                ne: if glyph == Glyph::Close {
                    BAND_PANEL_RADIUS
                } else {
                    0
                },
                se: if glyph == Glyph::Add && add_available {
                    BAND_PANEL_RADIUS
                } else {
                    0
                },
                ..egui::CornerRadius::ZERO
            };
            p.rect_filled(
                rect,
                rounding,
                Color32::from_white_alpha(15).gamma_multiply(hover),
            );
        }
        let color = if glyph == Glyph::Listen && panel_listening(ui, &response) {
            let palette = theme::current(ui.ctx());
            match target {
                modifiers::ModifierTarget::Band(index) => palette.band[index],
                modifiers::ModifierTarget::Region(index) => palette.region[index],
            }
        } else if enabled {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        icons::draw(
            p,
            ui.ctx(),
            glyph,
            Rect::from_center_size(rect.center(), Vec2::splat(BAND_ACTION_GLYPH)),
            color,
        );
        response
    })
}

fn close_tooltip(ui: &egui::Ui, response: &Response) {
    let id = response.id.with("close-tooltip");
    let now = ui.input(|input| input.time);

    if !response.hovered() || ui.input(|input| input.pointer.any_down()) {
        ui.data_mut(|data| data.remove::<f64>(id));
        return;
    }

    let started = ui.data_mut(|data| *data.get_temp_mut_or_insert_with(id, || now));
    let remaining = 0.1 - (now - started);

    if remaining > 0.0 {
        ui.ctx().request_repaint_after_secs(remaining as f32);
        return;
    }

    let modifier = if ui.ctx().os() == egui::os::OperatingSystem::Mac {
        "Command"
    } else {
        "Ctrl"
    };

    egui::Tooltip::for_widget(response).show(|ui| {
        ui.label(format!(
            "Click to close this panel. Hold {modifier} and left click to delete this point."
        ));
    });
}

pub(in crate::editor) fn delete_point_clicked(ui: &egui::Ui, close: &Response) -> bool {
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    close.clicked_by(egui::PointerButton::Primary)
        && ui.input(|input| {
            if mac {
                input.modifiers.mac_cmd
            } else {
                input.modifiers.ctrl
            }
        })
}

pub(in crate::editor) fn panel_listening(ui: &egui::Ui, response: &Response) -> bool {
    response.is_pointer_button_down_on()
        && ui.input(|input| input.focused && input.pointer.primary_down())
}

pub(in crate::editor) fn panel_power(
    ui: &mut egui::Ui,
    panel: Rect,
    target: modifiers::ModifierTarget,
    on: bool,
    accent: Color32,
) -> Response {
    let rect = Rect::from_min_size(panel.min + Vec2::new(14.0, 8.0), Vec2::splat(20.0));
    let response = ui
        .interact(
            rect,
            ui.make_persistent_id(("panel-power", target)),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let pressed = response.is_pointer_button_down_on();
    let highlight = if pressed {
        1.0
    } else {
        hover_amount(&response)
    };
    let p = ui.painter();
    if highlight > 0.0 {
        p.circle_filled(
            rect.center(),
            10.0,
            Color32::from_gray(if pressed { 40 } else { 58 }).gamma_multiply(highlight),
        );
        p.circle_stroke(
            rect.center(),
            10.0,
            Stroke::new(1.0, Color32::from_gray(92).gamma_multiply(highlight)),
        );
    }
    power_icon(
        p,
        rect.center(),
        if on { lighten(accent, 0.35) } else { ui.visuals().weak_text_color() },
        18.0,
    );
    response
}

impl EquartEditor {
    pub(in crate::editor) fn band_panel(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        if self.band_dropdown_owner != self.selected {
            self.band_dropdowns.fill(CardLife::SHUT);
            self.band_dropdown_owner = self.selected;
        }
        let Some(index) = self.selected else { return };
        let p = ui.painter_at(canvas);
        let panel = self.band_panel_rect(ui, canvas);

        panel_plate(&p, panel);

        let params = self.params.clone();
        let band = params.band(index);
        if modifiers::Modifier::BAND
            .iter()
            .any(|modifier| modifier.attached(band))
        {
            self.band_panel_scroll(ui, panel, index, band);
        } else {
            self.band_panel_contents(ui, panel, index);
        }
    }

    pub(in crate::editor) fn band_panel_rect(&self, ui: &egui::Ui, canvas: Rect) -> Rect {
        let mut rect = Layout::panel(canvas, self.panel_anchor_x(ui, canvas));
        let extra = self.selected.map_or(0.0, |index| {
            modifiers::Modifier::BAND
                .into_iter()
                .filter(|modifier| modifier.attached(self.params.band(index)))
                .map(modifiers::Modifier::width)
                .sum()
        });
        if extra > 0.0 {
            let width = (Layout::PANEL_W + extra).min(canvas.width() - 24.0);
            let left = rect.left().min(canvas.right() - 12.0 - width);
            rect.min.x = left;
            rect.max.x = left + width;
        }
        rect
    }

    fn band_panel_scroll(
        &mut self,
        ui: &mut egui::Ui,
        panel: Rect,
        index: usize,
        band: &crate::params::BandParams,
    ) {
        let attached = || {
            modifiers::Modifier::BAND
                .into_iter()
                .filter(|modifier| modifier.attached(band))
        };
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(panel)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(panel));
                let area = egui::ScrollArea::horizontal()
                    .id_salt(("band-controls", index))
                    .auto_shrink([false, false])
                    .max_height(panel.height());

                theme::scroll(ui, area, self.theme.scrollbar, |content| {
                    let origin = content.cursor().min;
                    content.set_min_size(Vec2::new(
                        Layout::PANEL_W + attached().map(modifiers::Modifier::width).sum::<f32>(),
                        panel.height() - 5.0,
                    ));
                    let base =
                        Rect::from_min_size(origin, Vec2::new(Layout::PANEL_W, panel.height()));
                    self.band_panel_contents(content, base, index);

                    if self.selected != Some(index) {
                        return;
                    }

                    let mut left = base.right_top();

                    for modifier in attached() {
                        let module = Rect::from_min_size(
                            left,
                            Vec2::new(modifier.width(), panel.height() - 5.0),
                        );

                        if self.reveal_modifier == Some((index, modifier)) {
                            content.scroll_to_rect(
                                Rect::from_min_size(module.min, Vec2::ZERO),
                                Some(egui::Align::Min),
                            );
                            self.reveal_modifier = None;
                        }

                        modifier.show(self, content, module, index);
                        left = module.right_top();
                    }
                });
            },
        );
    }

    fn band_panel_contents(&mut self, ui: &mut egui::Ui, panel: Rect, index: usize) {
        let params = self.params.clone();
        let band = params.band(index);
        let accent = self.theme.band[index];
        let p = ui.painter().clone();
        let rel = |x: f32, y: f32| panel.min + Vec2::new(x, y);
        let bold = |size: f32| FontId::new(size, egui::FontFamily::Name(GLOBAL_FONT.into()));

        let target = modifiers::ModifierTarget::Band(index);
        let destination = self.free_region_slot();
        let [close, listen, switch, add] = panel_actions(
            ui,
            panel,
            target,
            modifiers::Modifier::available_on(band),
            destination.is_some(),
        );
        if delete_point_clicked(ui, &close) {
            self.delete_band(index);
            return;
        }
        if close.clicked() {
            self.clear_selection();
            return;
        }
        if panel_listening(ui, &listen) {
            self.held_audition = Some(crate::engine::audition::Target::Band(index));
        }
        if switch.clicked()
            && let Some(destination) = destination
        {
            self.request_band_to_region(index, destination);
            return;
        }
        self.show_modifier_picker(&add, target);

        let nav = |x: f32| Rect::from_min_size(rel(x, 7.0), Vec2::splat(22.0));

        let prev = nav(38.0);
        let next = nav(80.0);

        for (rect, salt, rightwards) in [
            (prev, ("panel-prev", 0), false),
            (next, ("panel-next", 0), true),
        ] {
            let response = ui
                .interact(rect, ui.make_persistent_id(salt), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);

            let hover = hover_amount(&response);
            if hover > 0.0 {
                p.rect_filled(rect, 3.0, Color32::from_gray(52).gamma_multiply(hover));
            }
            caret_icon(
                &p,
                rect.center(),
                self.theme.panel.muted_label.lerp_to_gamma(self.theme.panel.label, hover),
                22.0,
                rightwards,
            );

            if response.clicked() {
                self.selected = self.step_to_band(index, rightwards);
            }
        }

        p.text(
            prev.center().lerp(next.center(), 0.5),
            egui::Align2::CENTER_CENTER,
            format!("{}", index + 1),
            bold(11.0),
            self.theme.panel.label,
        );

        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();

        let on = band.enabled.value();
        let power_response = panel_power(ui, panel, target, on, accent);
        if power_response.clicked() {
            if !on {

                self.params.mark_placed(index);
            }
            self.set(&setter, &band.enabled, !on);
        }

        let combo_rect = |row: f32| {
            Rect::from_min_size(
                rel(14.0, 36.0 + row * (menu::COMBO_BUTTON_H + 4.0)),
                Vec2::new(110.0, menu::COMBO_BUTTON_H),
            )
        };
        let kind = band.kind.value();
        if let Some(kind) = menu::combo_popup(
            ui,
            ui.make_persistent_id(("panel-kind", index)),
            combo_rect(0.0),
            kind,
            &FilterKind::ALL,
            FilterKind::label,
            accent,
            &mut self.band_dropdowns[0],
        ) {
            self.set(&setter, &band.kind, kind);
            let snapped = kind.effective_slope(band.slope.value());
            if snapped != band.slope.value() {
                self.set(&setter, &band.slope, snapped);
            }
        }

        let current_slope = kind.effective_slope(band.slope.value());
        let slopes = kind.slopes();
        if let Some(slope) = menu::combo_popup(
            ui,
            ui.make_persistent_id(("panel-slope", index)),
            combo_rect(1.0),
            current_slope,
            slopes,
            |slope| {
                if slopes.is_empty() {
                    "Slope".to_owned()
                } else {
                    slope.label()
                }
            },
            accent,
            &mut self.band_dropdowns[1],
        ) {
            self.set(&setter, &band.slope, slope);
        }

        if let Some(mode) = menu::combo_popup(
            ui,
            ui.make_persistent_id(("panel-channel", index)),
            combo_rect(2.0),
            band.channel.value(),
            &crate::band::ChannelMode::ALL,
            crate::band::ChannelMode::label,
            accent,
            &mut self.band_dropdowns[2],
        ) {
            self.set(&setter, &band.channel, mode);
        }

        // 3 knobs here
        let q_enabled = band.kind.value().uses_q(band.slope.value());
        let frequency_mode = if band.pitch.enabled.value() {
            let hz = if band.pitch.bypassed.value()
                || !band.enabled.value()
                || params.global.bypass.value()
            {
                band.frequency.value()
            } else {
                self.telemetry
                    .tracked_frequency(index)
                    .unwrap_or_else(|| band.frequency.value())
            };
            knob::KnobMode::ReadOnly(hz)
        } else {
            knob::KnobMode::Editable
        };
        let gain_mode = if band.dynamics.enabled.value()
            && band.enabled.value()
            && !params.global.bypass.value()
        {
            let (_, offset) = self.telemetry.dynamic_meter(index);
            knob::KnobMode::GainOffset(offset)
        } else {
            knob::KnobMode::Editable
        };
        let knobs = [
            (&band.frequency, "FREQ", 192.0, true, true, frequency_mode),
            (
                &band.gain,
                "GAIN",
                280.0,
                true,
                band.kind.value().has_gain(),
                gain_mode,
            ),
            (
                &band.q,
                "Q",
                368.0,
                false,
                q_enabled,
                knob::KnobMode::Editable,
            ),
        ];

        for (param, label, x, bipolar, enabled, mode) in knobs {
            let center = rel(x, 68.0);
            let builder = egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(center, Vec2::new(110.0, 104.0)));
            let builder = if enabled { builder } else { builder.disabled() };
            ui.scope_builder(builder, |ui| {
                ui.visuals_mut().selection.bg_fill = accent.gamma_multiply(0.3);
                ui.visuals_mut().selection.stroke = Stroke::new(1.0, accent);
                let read_only = matches!(mode, knob::KnobMode::ReadOnly(_));
                if read_only {
                    ui.multiply_opacity(0.55);
                }
                let response = knob::knob(
                    ui,
                    param,
                    &setter,
                    knob::KnobAppearance {
                        center,
                        radius: 24.0,
                        grow: 1.05,
                        body_fill: 1.06,
                        label_gap: 40.0,
                        label,
                        bipolar,
                    },
                    mode,
                );
                if read_only {
                    response.on_hover_text("Frequency is controlled by f0lock.");
                }
            });
        }
    }

    pub(in crate::editor) fn band_dropdown_is_up(&self) -> bool {
        self.modifier_picker
            .as_ref()
            .is_some_and(modifiers::Picker::is_up)
            || (self.band_dropdown_owner == self.selected
                && self.band_dropdowns.iter().any(CardLife::is_up))
    }

    pub(in crate::editor) fn band_menu(&mut self, ui: &mut egui::Ui, index: usize) {
        let params = self.params.clone();
        let band = params.band(index);

        ui.label(format!("Band {}", index + 1));
        ui.separator();

        if ui
            .selectable_label(band.enabled.value(), "Enabled")
            .clicked()
        {
            self.set_band_enabled(index, !band.enabled.value());
        }

        ui.menu_button("Type", |ui| {
            let Some(gui) = &self.gui else { return };
            let setter = gui.param_setter();
            for kind in FilterKind::ALL {
                if ui
                    .selectable_label(band.kind.value() == kind, kind.label())
                    .clicked()
                {
                    self.set(&setter, &band.kind, kind);
                    let snapped = kind.effective_slope(band.slope.value());
                    if snapped != band.slope.value() {
                        self.set(&setter, &band.slope, snapped);
                    }
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });

        ui.menu_button("Channel", |ui| {
            let Some(gui) = &self.gui else { return };
            let setter = gui.param_setter();
            for mode in crate::band::ChannelMode::ALL {
                if ui
                    .selectable_label(band.channel.value() == mode, mode.label())
                    .clicked()
                {
                    self.set(&setter, &band.channel, mode);
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });

        let kind = band.kind.value();
        if !kind.slopes().is_empty() {
            ui.menu_button("Slope", |ui| {
                let Some(gui) = &self.gui else { return };
                let setter = gui.param_setter();
                let current_slope = kind.effective_slope(band.slope.value());
                for &slope in kind.slopes() {
                    if ui
                        .selectable_label(current_slope == slope, slope.label())
                        .clicked()
                    {
                        self.set(&setter, &band.slope, slope);
                        egui::Popup::close_all(ui.ctx());
                    }
                }
            });
        }

        ui.separator();

        if ui.button("Reset gain and Q").clicked() {
            let Some(gui) = &self.gui else { return };
            let setter = gui.param_setter();
            self.set(&setter, &band.gain, 0.0);
            self.set(&setter, &band.q, 1.0);
            egui::Popup::close_all(ui.ctx());
        }

        if ui.button("Copy to all bands").clicked() {
            let Some(gui) = &self.gui else { return };
            let setter = gui.param_setter();
            let kind = band.kind.value();
            let q = band.q.value();
            for (oidx, other) in params.bands.iter().enumerate() {
                if oidx == index || !self.band_exists(oidx) {
                    continue;
                }

                params.mark_placed(oidx);
                self.set(&setter, &other.kind, kind);
                self.set(&setter, &other.q, q);
            }
            egui::Popup::close_all(ui.ctx());
        }
    }
}
