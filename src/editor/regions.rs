use super::*;
use crate::params::FREQUENCY_MAX_HZ;
use crate::region::RegionControl;
use spectrum::Plot;

pub(super) const REGION_COLOR: Color32 = Color32::from_rgb(113, 161, 225);
const REGION_PANEL_WIDTH: f32 = 344.0;

impl EquartEditor {
    pub(super) fn draw_regions(
        &mut self,
        ui: &mut egui::Ui,
        plot: &Plot,
        regions: &[RegionControl],
    ) {
        let painter = ui.painter_at(plot.rect);
        for (row, (index, region)) in regions
            .iter()
            .enumerate()
            .filter(|(_, region)| region.enabled)
            .enumerate()
        {
            let [left, right] = region.bounds().map(|hz| plot.x(hz));
            let selected = self.selected_region == Some(index);
            let fill: u8 = if region.bypass {
                9
            } else if selected {
                37
            } else {
                22
            };
            let color = if region.bypass { MUTED } else { REGION_COLOR };
            let rect = Rect::from_x_y_ranges(left..=right, plot.rect.y_range());
            painter.rect_filled(rect, 0.0, color.gamma_multiply(f32::from(fill) / 255.0));
            for edge in [left, right] {
                painter.vline(
                    edge,
                    rect.y_range(),
                    Stroke::new(1.0, color.gamma_multiply(0.4)),
                );
            }

            let x = ((left + right) * 0.5).clamp(plot.rect.left() + 39.0, plot.rect.right() - 39.0);
            let y = plot.rect.top() + 16.0 + row as f32 * 22.0;
            let chip = Rect::from_center_size(Pos2::new(x, y), Vec2::new(72.0, 19.0));
            let button = ui
                .interact(
                    chip,
                    ui.make_persistent_id(("spectral-region", index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            painter.rect_filled(chip, 5.0, Color32::from_gray(32));
            painter.rect_stroke(
                chip,
                5.0,
                Stroke::new(
                    1.0,
                    color.gamma_multiply(if selected {
                        0.9
                    } else {
                        0.4 + 0.4 * hover_amount(&button)
                    }),
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                format!("Region {}", index + 1),
                FontId::proportional(11.0),
                color,
            );
            if button.clicked() || button.secondary_clicked() {
                self.select_region(index);
            }
            button.context_menu(|ui| {
                if ui.selectable_label(region.bypass, "Bypass").clicked() {
                    if let Some(gui) = &self.gui {
                        self.set(
                            &gui.param_setter(),
                            &self.params.regions[index].bypass,
                            !region.bypass,
                        );
                    }
                    ui.close();
                }
                if ui.button("Delete Region").clicked() {
                    self.delete_region(index);
                    ui.close();
                }
            });
        }
    }

    pub(super) fn place_region(&mut self, hz: f32) {
        let Some(gui) = &self.gui else { return };
        let Some(index) = self.free_region_slot() else {
            return;
        };
        let min = crate::analyzer::MIN_HZ;
        let control = RegionControl {
            enabled: true,
            low: (hz / std::f32::consts::SQRT_2).max(min),
            high: (hz * std::f32::consts::SQRT_2).min(FREQUENCY_MAX_HZ),
            ..Default::default()
        };
        self.write_region(&gui.param_setter(), index, control);
        self.select_region(index);
    }

    pub(super) fn free_region_slot(&self) -> Option<usize> {
        self.params
            .regions
            .iter()
            .position(|region| !region.enabled.value())
    }

    fn write_region(&self, setter: &ParamSetter<'_>, index: usize, control: RegionControl) {
        let region = &self.params.regions[index];
        self.set(setter, &region.low, control.low);
        self.set(setter, &region.high, control.high);
        self.set(setter, &region.softness, control.softness);
        self.set(setter, &region.gain, control.gain);
        self.set(setter, &region.bypass, control.bypass);

        let res = &region.resonance;
        self.set(setter, &res.enabled, control.resonance.enabled);
        self.set(setter, &res.bypassed, false);
        self.set(setter, &res.depth, control.resonance.depth);
        self.set(setter, &res.threshold, control.resonance.threshold);
        self.set(setter, &res.q, control.resonance.q);
        self.set(setter, &res.attack, control.resonance.attack_ms);
        self.set(setter, &res.release, control.resonance.release_ms);
        self.set(setter, &res.ratio, control.resonance.ratio);
        self.set(setter, &res.smooth, control.resonance.smooth);
        self.set(setter, &res.stereo_link, control.resonance.stereo_link);
        self.set(setter, &res.bias, control.resonance.bias);
        self.set(setter, &region.harmonic.enabled, control.harmonic.enabled);
        self.set(setter, &region.harmonic.bypassed, false);
        self.set(setter, &region.harmonic.mode, control.harmonic.mode);
        self.set(setter, &region.harmonic.first, control.harmonic.first);
        self.set(setter, &region.harmonic.last, control.harmonic.last);
        self.set(setter, &region.enabled, control.enabled);
    }

    fn conversion_rate(&self) -> f32 {
        let rate = self.telemetry.sample_rate();
        let rate = if rate > 0.0 { rate } else { 48_000.0 };
        rate * self.params.oversampling.value().factor() as f32
    }

    pub(super) fn convert_band_to_region(&mut self, index: usize, destination: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let control = RegionControl::from_band(self.control(index), self.conversion_rate());
        self.reset_band(&setter, index);
        self.write_region(&setter, destination, control);
        self.select_region(destination);
    }

    fn convert_region_to_band(&mut self, index: usize, destination: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let control = self.params.regions[index]
            .control()
            .to_band(self.conversion_rate());
        self.reset_region(&setter, index);
        self.reset_band(&setter, destination);
        let band = self.params.band(destination);
        self.set(&setter, &band.kind, control.kind);
        self.set(&setter, &band.frequency, control.frequency);
        self.set(&setter, &band.gain, control.gain);
        self.set(&setter, &band.q, control.q);
        self.set(&setter, &band.slope, control.slope);
        self.params.mark_placed(destination);
        self.set(&setter, &band.enabled, control.enabled);
        self.clear_selection();
        self.pick_only(destination);
    }

    fn select_region(&mut self, index: usize) {
        self.clear_selection();
        self.selected_region = Some(index);
    }

    fn delete_region(&mut self, index: usize) {
        let Some(gui) = &self.gui else { return };
        self.reset_region(&gui.param_setter(), index);
        self.clear_selection();
    }

    pub(super) fn reset_region(&self, setter: &ParamSetter<'_>, index: usize) {
        let region = &self.params.regions[index];
        self.set(setter, &region.enabled, false);
        self.set(setter, &region.bypass, false);
        self.set(setter, &region.resonance.enabled, false);
        self.set(setter, &region.resonance.bypassed, false);
        let harmonic = &region.harmonic;
        self.set(setter, &harmonic.enabled, false);
        self.set(setter, &harmonic.bypassed, false);
        self.set(setter, &harmonic.mode, harmonic.mode.default_plain_value());
        self.set(
            setter,
            &harmonic.first,
            harmonic.first.default_plain_value(),
        );
        self.set(setter, &harmonic.last, harmonic.last.default_plain_value());
        for param in [
            &region.low,
            &region.high,
            &region.softness,
            &region.gain,
            &region.resonance.depth,
            &region.resonance.threshold,
            &region.resonance.q,
            &region.resonance.attack,
            &region.resonance.release,
            &region.resonance.ratio,
            &region.resonance.smooth,
            &region.resonance.stereo_link,
            &region.resonance.bias,
        ] {
            let default = param.default_plain_value();
            if param.value() != default {
                self.set(setter, param, default);
            }
        }
    }

    pub(super) fn region_panel_rect(&self, canvas: Rect, rate: f32) -> Option<Rect> {
        let index = self.selected_region?;
        let control = self.params.regions[index].control();
        if !control.enabled {
            return None;
        }
        let [low, high] = control.bounds();
        let plot = Plot::new(Layout::plot_outer(canvas), rate);
        Some(Layout::panel_with_width(
            canvas,
            plot.x((low * high).sqrt()),
            self.region_panel_width(index).min(canvas.width() - 24.0),
        ))
    }

    fn region_panel_width(&self, index: usize) -> f32 {
        REGION_PANEL_WIDTH
            + modifiers::Modifier::REGION.into_iter()
                .filter(|m| m.attached_to_region(&self.params.regions[index]))
                .map(modifiers::Modifier::width).sum::<f32>()
    }

    pub(super) fn region_panel(&mut self, ui: &mut egui::Ui, canvas: Rect, rate: f32) {
        let Some(index) = self.selected_region else {
            return;
        };
        let Some(panel) = self.region_panel_rect(canvas, rate) else {
            self.clear_selection();
            return;
        };
        let width = self.region_panel_width(index);
        controls::panel_plate(&ui.painter_at(canvas), panel);
        if width <= canvas.width() - 24.0 {
            self.region_panel_contents(ui, panel, index);
            return;
        }
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(panel)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(panel));
                egui::ScrollArea::horizontal()
                    .id_salt(("region-controls", index))
                    .auto_shrink([false, false])
                    .max_height(panel.height())
                    .show(ui, |content| {
                        let origin = content.cursor().min;
                        content.set_min_size(Vec2::new(width, panel.height() - 5.0));
                        self.region_panel_contents(
                            content,
                            Rect::from_min_size(origin, Vec2::new(width, panel.height())),
                            index,
                        );
                    });
            },
        );
    }

    fn region_panel_contents(&mut self, ui: &mut egui::Ui, panel: Rect, index: usize) {
        let params = self.params.clone();
        let region = &params.regions[index];
        let base = Rect::from_min_size(panel.min, Vec2::new(REGION_PANEL_WIDTH, panel.height()));
        let target = modifiers::ModifierTarget::Region(index);
        let destination = (0..BAND_COUNT).find(|&index| !self.band_exists(index));
        let [close, delete, switch, add] = controls::panel_actions(
            ui,
            base,
            target,
            modifiers::Modifier::REGION.iter().any(|m| !m.attached_to_region(region)),
            destination.is_some(),
        );
        if close.clicked() {
            self.clear_selection();
            return;
        }
        if delete.clicked() {
            self.delete_region(index);
            return;
        }
        if switch.clicked()
            && let Some(destination) = destination
        {
            self.convert_region_to_band(index, destination);
            return;
        }
        self.show_modifier_picker(&add, target);
        let mut origin = base.right_top();
        for modifier in modifiers::Modifier::REGION.into_iter().filter(|m| m.attached_to_region(region)) {
            let module = Rect::from_min_size(origin, Vec2::new(modifier.width(), panel.height()));
            origin = module.right_top();
            if self.reveal_modifier == Some((index, modifier)) {
                ui.scroll_to_rect(module, Some(egui::Align::Max));
                self.reveal_modifier = None;
            }
            modifier.show(self, ui, module, index);
        }
        ui.painter().text(
            panel.min + Vec2::new(44.0, 18.0),
            egui::Align2::LEFT_CENTER,
            format!("Spectral Region {}", index + 1),
            FontId::new(11.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
            TEXT,
        );
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let bypass = region.bypass.value();
        if controls::panel_power(ui, base, target, !bypass, REGION_COLOR)
            .on_hover_text(if bypass {
                "Enable region"
            } else {
                "Bypass region"
            })
            .clicked()
        {
            self.set(&setter, &region.bypass, !bypass);
        }
        let parameters = Rect::from_min_max(
            panel.min + Vec2::new(16.0, 36.0),
            base.max - Vec2::new(controls::BAND_ACTION_WIDTH + 12.0, 8.0),
        );
        let label = |ui: &mut egui::Ui, text| ui.add_sized([98.0, 20.0], egui::Label::new(text));
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("region-panel", index))
                .max_rect(parameters)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    label(ui, "Range")
                        .on_hover_text("The range endpoints are at half the set gain.");
                    self.region_number(ui, &setter, &region.low, 1.0);
                    ui.label("–");
                    self.region_number(ui, &setter, &region.high, 1.0);
                });
                ui.horizontal(|ui| {
                    label(ui, "Edge Softness").on_hover_text("Widen the transition at each edge.");
                    self.region_number(ui, &setter, &region.softness, 0.3);
                });
                ui.horizontal(|ui| {
                    label(ui, "Gain");
                    self.region_number(ui, &setter, &region.gain, 0.1);
                });
            },
        );
    }

    fn region_number(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        param: &FloatParam,
        speed: f32,
    ) {
        let mut value = param.value();
        let range = param.range();
        if ui
            .add(
                egui::DragValue::new(&mut value)
                    .range(range.unnormalize(0.0)..=range.unnormalize(1.0))
                    .suffix(param.unit())
                    .speed(speed)
                    .max_decimals(1),
            )
            .changed()
        {
            self.set(setter, param, value);
        }
    }
}
