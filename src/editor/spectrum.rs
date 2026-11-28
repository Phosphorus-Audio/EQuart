use super::*;
use crate::analyzer::{
    AnalyzerOption, HOLD_MODES, MIN_HZ, RESOLUTION_LABELS, SPECTRUM_SMOOTHINGS,
    SPECTRUM_SPEED_LABELS, SpectrumFrame, TILT_LABELS, max_frequency,
};
use crate::band::{BAND_COUNT, BandControl, SECTIONS};
use crate::editor::format_hz;
use crate::filter::{Coefficients, CutSlope, FilterKind, bell_bandwidth};
use crate::params::{Q_MAX, Q_MIN};
use crate::region::{REGION_COUNT, RegionControl};
use realfft::num_complex::Complex32;

mod backdrop;
mod band_input;
mod fill;
mod grid;
pub(super) mod heatmap;
mod keys;
mod meter;
mod panel_motion;
mod phase;
mod plot;
mod response;
mod settings;
mod trace;

use super::wheel::{take_steps, wheel_steps};
use backdrop::{ease_glow, edge_fade, soft_wash};
pub(crate) use band_input::{BandDrag, cut_handle_q, cut_handle_y};
use band_input::{cut_handle_db, cut_q, follow, key_hz};
pub(super) use fill::gradient_fill;
use grid::{draw_axis_labels, draw_grid_lines};
use keys::{draw_keys, note_at, note_hz};
pub(crate) use plot::Plot;
use response::{ResponseInput, draw_response};
use trace::draw_spectrum;

pub(crate) use keys::keys_strip;

/// keyboard strip height + top gap
pub(crate) const KEYS_BAND: f32 = 26.0;
const KEYS_HEIGHT: f32 = 27.0;
/// fade width at both ends of keyboard
const KEYS_FADE: f32 = 78.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AxisScale {
    Eq,
    Spectrum,
}

/// peak hold mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
    Off,
    Line,
    Smooth,
}

impl Hold {
    fn from_index(index: u32) -> Self {
        match index {
            0 => Self::Off,
            2 => Self::Smooth,
            _ => Self::Line,
        }
    }
}

/// axis hover/press wash alphas
const AXIS_HOVER_ALPHA: f32 = 0.09;
const AXIS_PRESS_ALPHA: f32 = 0.16;
const AXIS_FADE_IN: f32 = 0.055;
const AXIS_FADE_OUT: f32 = 0.20;

impl EquartEditor {
    pub(super) fn spectrum(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        rate: f32,
        spectrum: &SpectrumFrame<'_>,
    ) {
        let outer = Layout::plot_outer(canvas, self.meter_visible());
        let p = ui.painter_at(outer);

        let preferences = self.display_preferences.read();
        let layout = preferences.layout;
        let piano_keys_visible = layout.piano_keys_visible;
        let axis_range = self.axis_range();
        let visual_range = ui.ctx().animate_value_with_time(
            ui.make_persistent_id("eq-axis-range"),
            axis_range,
            0.16,
        );
        let plot = Plot::with_ranges(
            outer,
            rate,
            visual_range,
            self.spectrum_range(),
            piano_keys_visible,
        );
        // hide hover note readout when dB dial is open
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|_| !self.dial.is_shown());
        let (eq_highlight, spectrum_highlight) = self.axis_controls(ui, &plot, outer);
        let bypass = self.params.global.bypass.value();
        let heatmap = preferences.spectrum.heatmap;

        if heatmap.above_grid {
            draw_grid_lines(&p, &plot, outer);
        }

        if !bypass {
            self.heatmap.draw(
                &p,
                &plot,
                spectrum,
                Hold::from_index(preferences.spectrum.index(AnalyzerOption::Hold)),
                heatmap,
                &self.theme,
            );
        }

        if !heatmap.above_grid {
            draw_grid_lines(&p, &plot, outer);
        }

        let brand_rect = draw_brand(ui, &p, &plot, !self.footer_menu_is_up());
        let piano_key_fade = layout.piano_key_fade;

        if bypass {
            // fade edges before watermark
            edge_fade(&p, outer, canvas, plot.rect);
            draw_axis_labels(
                &p,
                &plot,
                outer,
                axis_range,
                eq_highlight,
                spectrum_highlight,
            );

            if piano_keys_visible {
                draw_keys(&p, &plot, outer, hover, self.accent(), piano_key_fade);
            }

            p.text(
                plot.rect.center(),
                egui::Align2::CENTER_CENTER,
                "BYPASS",
                FontId::new(80.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
                Color32::from_white_alpha(36),
            );
            self.brand_click(ui, brand_rect);
            return;
        }

        draw_spectrum(&p, &plot, outer, spectrum, self.hold());

        // use live tracked pitch for display if enabled
        let controls = std::array::from_fn(|i| self.control(i));
        let mut display_controls = controls;
        for (index, control) in display_controls.iter_mut().enumerate() {
            if control.enabled
                && control.pitch.enabled
                && let Some(hz) = self.telemetry.tracked_frequency(index)
            {
                control.frequency = hz;
            }
        }
        let markers: Vec<(usize, Pos2)> = display_controls
            .iter()
            .enumerate()
            .filter(|(index, _)| self.band_exists(*index))
            .map(|(i, control)| {
                // cut handle y uses Q since cuts have no gain
                let y = if control.kind.is_cut() {
                    cut_handle_y(&plot, cut_handle_q(control))
                } else {
                    plot.y(control.gain)
                };
                (i, Pos2::new(plot.x(control.frequency), y))
            })
            .collect();

        for (index, control) in display_controls.iter_mut().enumerate() {
            if (control.mapping.enabled || control.lfo.enabled)
                && let Some(live) = self.telemetry.modulation(index)
            {
                live.apply(control);
            }
        }

        let response = ui.interact(
            plot.rect,
            ui.make_persistent_id("eq-plot"),
            Sense::click_and_drag(),
        );
        let regions: [RegionControl; REGION_COUNT] =
            std::array::from_fn(|index| self.params.regions[index].control());
        self.draw_regions(ui, &plot, &regions);

        // find nearest band dot within 16px
        let point_at = |position: Pos2| {
            markers
                .iter()
                .map(|&(i, point)| (i, point.distance(position)))
                .filter(|(_, distance)| *distance <= 16.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };

        let pointer = response.interact_pointer_pos();
        let nearest = pointer.and_then(point_at);
        let pressed_at = ui.input(|i| i.pointer.press_origin());
        let pressed_band_id = response.id.with("pressed-band");
        let curve_preview_id = response.id.with("curve-preview");
        let pressed_preview_id = response.id.with("pressed-preview");

        if ui.input(|i| i.pointer.primary_pressed()) {
            let pressed = pressed_at
                .and_then(point_at)
                .filter(|_| response.is_pointer_button_down_on())
                .map(|i| (i, controls[i].enabled && controls[i].pitch.enabled));
            ui.data_mut(|data| data.insert_temp(pressed_band_id, pressed));

            let last_preview = ui
                .data(|data| data.get_temp::<Option<Pos2>>(curve_preview_id))
                .flatten();
            let hit_preview = pressed
                .is_none()
                .then(|| {
                    pressed_at.and_then(|pos| {
                        last_preview.filter(|preview| {
                            response.is_pointer_button_down_on() && preview.distance(pos) <= 16.0
                        })
                    })
                })
                .flatten();
            ui.data_mut(|data| data.insert_temp(pressed_preview_id, hit_preview));
        }

        let press_target = ui
            .data(|data| data.get_temp::<Option<(usize, bool)>>(pressed_band_id))
            .flatten();
        // keep tracked band from press pos in case it moved
        let pressed_band = press_target.and_then(|(index, tracked)| tracked.then_some(index));
        let clicked_band = pressed_band.or(nearest);

        if response.clicked()
            && let Some(index) = clicked_band
        {
            if ui.input(|i| i.modifiers.alt) {
                self.set_band_enabled(index, !controls[index].enabled);
            }
            self.pick_only(index);
        }

        // don't treat clicks on panel or popups as blank plot clicks
        let panel = self
            .region_panel_rect(canvas, rate)
            .or_else(|| self.selected.map(|_| self.band_panel_rect(ui, canvas)));
        let panel_pointer = pointer.or_else(|| ui.input(|input| input.pointer.hover_pos()));
        let over_panel = panel_pointer
            .is_some_and(|position| panel.is_some_and(|panel| panel.contains(position)));
        let popup_open = egui::Popup::is_any_open(ui.ctx())
            || self.band_dropdown_is_up()
            || self.footer_menu_is_up()
            || self.dial.is_shown();

        if !popup_open
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            if let Some(region) = self.selected_region {
                self.delete_region(region);
            } else {
                self.delete_selected_bands();
            }
        }

        if response.clicked()
            && !response.double_clicked()
            && clicked_band.is_none()
            && !over_panel
            && !popup_open
        {
            self.clear_selection();
        }

        if response.double_clicked() && !over_panel && !popup_open {
            match (clicked_band, pointer) {
                (Some(index), _) if controls[index].kind.has_gain() => {
                    if let Some(gui) = &self.gui {
                        let setter = gui.param_setter();
                        self.set(&setter, &self.params.bands[index].gain, 0.0);
                    }
                }
                (None, Some(position)) => {
                    self.place_band(
                        plot.hz(position.x),
                        plot.db(position.y),
                        &controls,
                        ui.input(|i| i.time),
                    );
                }
                _ => {}
            }
        }

        if piano_keys_visible {
            // double click key to add band at 0 dB
            let keys = ui
                .interact(
                    keys_strip(outer),
                    ui.make_persistent_id("eq-keys"),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);

            if keys.double_clicked()
                && !popup_open
                && let Some(position) = keys
                    .interact_pointer_pos()
                    .or_else(|| ui.input(|i| i.pointer.hover_pos()))
            {
                self.place_band(
                    key_hz(&plot, position.x),
                    0.0,
                    &controls,
                    ui.input(|i| i.time),
                );
            }
        }

        if response.drag_started_by(egui::PointerButton::Primary) {
            match pressed_band.or_else(|| pressed_at.and_then(point_at)) {
                Some(index) => {
                    let marker = markers
                        .iter()
                        .find(|&&(i, _)| i == index)
                        .map(|&(_, point)| point);
                    let position = pressed_at.or(marker).unwrap_or(Pos2::ZERO);

                    // keep marquee group if already picked, otherwise pick only this one
                    if self.is_picked(index) {
                        self.picked[index] = true;
                        self.selected = Some(index);
                    } else {
                        self.pick_only(index);
                    }

                    // disabled bands can be selected, not dragged
                    if controls[index].enabled {
                        let followers = controls
                            .iter()
                            .enumerate()
                            .filter(|(other, _)| *other != index && self.is_picked(*other))
                            .map(|(other, control)| (other, (control.frequency, control.gain)))
                            .collect();
                        let grab = marker.map_or(Vec2::ZERO, |point| point - position);
                        self.begin_band_drag(
                            index,
                            position,
                            grab,
                            &controls[index],
                            followers,
                            axis_range,
                            plot.range,
                        );
                    }
                }
                // drag on curve preview creates and drags a new band immediately
                None if !over_panel && !popup_open => {
                    let hit_preview = ui
                        .data(|data| data.get_temp::<Option<Pos2>>(pressed_preview_id))
                        .flatten();

                    if let Some(point) = hit_preview
                        && let Some(index) = self.place_band(
                            plot.hz(point.x),
                            plot.db(point.y),
                            &controls,
                            ui.input(|i| i.time),
                        )
                    {
                        let created = self.control(index);
                        let start = pressed_at.unwrap_or(point);
                        self.begin_band_drag(
                            index,
                            start,
                            point - start,
                            &created,
                            Vec::new(),
                            axis_range,
                            plot.range,
                        );
                    } else if let Some(start) = pressed_at.or(pointer) {
                        self.marquee = Some((start, start));
                        self.end_band_drag();
                    }
                }
                None => {}
            }
        }

        if response.dragged_by(egui::PointerButton::Primary) {
            if let Some((start, _)) = self.marquee {
                if let Some(position) = ui.input(|i| i.pointer.interact_pos()) {
                    self.marquee = Some((start, position));
                }
            } else if let Some(drag) = &self.drag
                && let Some(raw_pos) = ui.input(|i| i.pointer.interact_pos())
                && let Some(gui) = &self.gui
            {
                let setter = gui.param_setter();
                let (shift, lock_axis) =
                    ui.input(|i| (i.modifiers.shift, i.modifiers.command));
                let delta = raw_pos - drag.last_pos;
                let step_scale = if shift { 0.15 } else { 1.0 };

                let mut virtual_pos = drag.virtual_pos + delta * step_scale;
                let origin_virtual = drag.origin_pos + drag.grab;

                if lock_axis {
                    let total = raw_pos - drag.origin_pos;
                    if total.x.abs() >= total.y.abs() {
                        virtual_pos.y = origin_virtual.y;
                    } else {
                        virtual_pos.x = origin_virtual.x;
                    }
                }

                let position = virtual_pos;
                let band = &self.params.bands[drag.band];
                let hz = if band.pitch.enabled.value() {
                    drag.origin.0
                } else {
                    let hz = plot.hz(position.x);
                    setter.set_parameter(&band.frequency, hz);
                    hz
                };

                let control = self.control(drag.band);
                let lead_has_gain = control.kind.has_gain();
                let adjusts_cut_q = control.kind.is_cut() && control.kind.uses_q(control.slope);

                let mut vertical = drag.vertical;
                let auto_db = if layout.auto_expand_eq_axis && (lead_has_gain || adjusts_cut_q) {
                    let limits = if lead_has_gain {
                        let range = band.gain.range();
                        range.unnormalize(0.0)..=range.unnormalize(1.0)
                    } else {
                        cut_handle_db(Q_MIN)..=cut_handle_db(Q_MAX)
                    };
                    let next_range = vertical.update(&plot, position.y, axis_range, limits, false);

                    if next_range != axis_range {
                        self.params
                            .axis_range_db
                            .store(next_range as u32, Ordering::Relaxed);
                        ui.ctx().request_repaint();
                    }

                    Some(vertical.db)
                } else {
                    None
                };

                let db = if lead_has_gain {
                    auto_db.unwrap_or_else(|| plot.db(position.y))
                } else {
                    let range = drag.origin_range;
                    (plot.db_free(position.y) * (range / plot.range)).clamp(-range, range)
                };

                if lead_has_gain {
                    setter.set_parameter(&band.gain, db);
                } else if adjusts_cut_q {
                    let db_delta = auto_db.map_or_else(
                        || plot.db_free(position.y) - plot.db_free(drag.origin_y),
                        |db| db - cut_handle_db(drag.origin_q),
                    );
                    let q = cut_q(&band.q, drag.origin_q, db_delta, false);
                    setter.set_parameter(&band.q, q);
                }

                for &(index, at_start) in &drag.followers {
                    let (moved_hz, moved_db) =
                        follow(drag.origin, (hz, db), at_start, lead_has_gain);
                    let follower = self.params.band(index);

                    if !follower.pitch.enabled.value() {
                        setter.set_parameter(&follower.frequency, moved_hz);
                    }

                    if self.control(index).kind.has_gain() {
                        setter.set_parameter(&self.params.bands[index].gain, moved_db);
                    }
                }

                if let Some(drag) = &mut self.drag {
                    drag.last_pos = raw_pos;
                    drag.virtual_pos = virtual_pos;
                    drag.vertical = vertical;
                }
            }
        }

        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.end_band_drag();
            if let Some((start, end)) = self.marquee.take() {
                self.finish_marquee(start, end, &markers);
            }
        }

        // wheel edits Q on bells or slope on cuts (works while dragging, hovering a node, or with a selected band)
        let wheel_target = self
            .drag
            .as_ref()
            .map(|drag| drag.band)
            .or_else(|| {
                if over_panel || popup_open {
                    return None;
                }

                response
                    .hover_pos()
                    .and_then(|position| point_at(position).or(self.selected))
            });
        let mut q_scale = None;
        let mut slope_change = None;

        if let Some(index) = wheel_target {
            let target_control = self.control(index);

            if target_control.kind.is_cut() {
                let travel = ui.input(|i| wheel_steps(&i.raw.events));
                let (steps, leftover) = take_steps(self.slope_wheel + travel);
                self.slope_wheel = leftover;

                if steps != 0 {
                    slope_change = Some((index, target_control.slope.step(steps)));
                }
            } else {
                self.slope_wheel = 0.0;
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);

                if scroll != 0.0 && target_control.kind.uses_q(target_control.slope) {
                    let fine = if ui.input(|i| i.modifiers.shift) {
                        0.2
                    } else {
                        1.0
                    };
                    q_scale = Some((index, 1.0 + scroll * 0.0025 * fine));
                }
            }
        } else {
            self.slope_wheel = 0.0;
        }

        if let Some((index, factor)) = q_scale {
            if let Some(gui) = &self.gui {
                let setter = gui.param_setter();
                let group_edit = self.is_picked(index);

                for other in 0..BAND_COUNT {
                    let apply = other == index || (group_edit && self.is_picked(other));
                    if !apply {
                        continue;
                    }

                    let control = self.control(other);
                    if control.kind.uses_q(control.slope) {
                        let q = (control.q * factor).clamp(Q_MIN, Q_MAX);
                        self.set(&setter, &self.params.bands[other].q, q);
                    }
                }
            }

            if self.drag.is_none() {
                self.selected_region = None;
                self.pending_region_selection = None;
                self.selected = Some(index);
            }
        }

        if let Some((index, slope)) = slope_change {
            if let Some(gui) = &self.gui {
                let setter = gui.param_setter();
                self.set(&setter, &self.params.bands[index].slope, slope);
            }

            if self.drag.is_none() {
                self.selected_region = None;
                self.pending_region_selection = None;
                self.selected = Some(index);
            }
        }

        #[derive(Clone, Copy)]
        enum PlotMenuTarget {
            Band(usize),
            Empty { hz: f32, db: f32 },
        }

        let target_id = response.id.with("plot-menu-target");
        let target = if response.secondary_clicked() {
            let target = if over_panel {
                egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(&response));
                None
            } else if let Some(index) = nearest {
                self.selected_region = None;
                self.pending_region_selection = None;
                self.selected = Some(index);
                Some(PlotMenuTarget::Band(index))
            } else {
                pointer.map(|position| PlotMenuTarget::Empty {
                    hz: plot.hz(position.x),
                    db: plot.db(position.y),
                })
            };
            ui.data_mut(|data| data.insert_temp(target_id, target));
            target
        } else if response.context_menu_opened() {
            ui.data(|data| data.get_temp::<Option<PlotMenuTarget>>(target_id))
                .flatten()
        } else {
            None
        };
        if let Some(target) = target {
            response.context_menu(|ui| match target {
                PlotMenuTarget::Band(index) => self.band_menu(ui, index),
                PlotMenuTarget::Empty { hz, db } => self.empty_plot_menu(ui, hz, db, &controls),
            });
        }

        // eval response at oversampled rate
        let processing = self.params.global.processing_mode.value();
        let oversampling = processing.oversampling(self.params.global.oversampling.value());
        let design_rate = if rate > 0.0 { rate } else { 48_000.0 } * oversampling.factor() as f32;
        let phase_mode = layout.phase_curve.then(|| {
            if layout.phase_unwrapped {
                phase::Mode::Unwrapped
            } else {
                phase::Mode::Wrapped
            }
        });
        let linear_phase = processing == crate::processing_mode::ProcessingMode::LinearPhase;
        if linear_phase {
            self.fir_response.update(&self.telemetry.fir);
        }
        let (pointer_down, primary_down) =
            ui.input(|input| (input.pointer.any_down(), input.pointer.primary_down()));
        let plot_hover = hover.filter(|&position| {
            !popup_open
                && !pointer_down
                && response.hovered()
                && !brand_rect.contains(position)
                && !panel.is_some_and(|rect| rect.contains(position))
        });
        let preview_hover = plot_hover.filter(|&position| {
            self.gui.is_some()
                && self.drag.is_none()
                && self.marquee.is_none()
                && point_at(position).is_none()
                && controls.iter().any(|control| !control.enabled)
        });
        let effective_regions = std::array::from_fn::<_, REGION_COUNT, _>(|index| {
            let region = self.telemetry.response_region(index, regions[index]);
            crate::region::response::whole(
                region,
                self.telemetry.harmonic_hz(index),
                if rate > 0.0 { rate } else { 48_000.0 },
            )
        });
        let preview = draw_response(
            &p,
            &plot,
            outer,
            &mut self.response_db,
            ResponseInput {
                controls: &display_controls,
                base: self.selected.map(|index| controls[index]),
                offsets: &self.telemetry.dynamics(),
                regions: &effective_regions,
                telemetry: &self.telemetry,
                rate: design_rate,
                oversampling,
                selected: self.selected,
                phase_mode,
                linear_center: linear_phase.then(|| {
                    self.params
                        .global
                        .linear_quality
                        .value()
                        .center_at(design_rate)
                }),
                fir_response: &self.fir_response,
                preview_hover,
            },
        );

        self.telemetry
            .region_dynamics_response
            .read_into(&mut self.region_dynamics_response);
        if self.region_dynamics_response.active()
            && rate > 0.0
            && self.params.global.mix.value() > 0.0
        {
            let curve = &self.region_dynamics_response;
            let points = (0..=256)
                .map(|i| {
                    let x = egui::lerp(plot.rect.x_range(), i as f32 / 256.0);
                    let db = curve.db(plot.hz_extended(x), rate);
                    Pos2::new(x, plot.y_free(db))
                })
                .collect::<Vec<_>>();
            if points
                .iter()
                .any(|point| (point.y - plot.y(0.0)).abs() > 0.7)
            {
                let painter = p.with_clip_rect(plot.rect);
                let ink = self.theme.meter.low;
                painter.extend(egui::Shape::dashed_line(
                    &points,
                    Stroke::new(1.6, ink),
                    6.0,
                    3.0,
                ));
            }
        }

        // fade plot edges before drawing labels
        edge_fade(&p, outer, canvas, plot.rect);
        draw_axis_labels(
            &p,
            &plot,
            outer,
            axis_range,
            eq_highlight,
            spectrum_highlight,
        );

        if piano_keys_visible {
            draw_keys(&p, &plot, outer, hover, self.accent(), piano_key_fade);
        }

        if let Some(mode) = phase_mode {
            phase::draw_label(&p, &plot, outer, mode);
        }

        ui.data_mut(|data| data.insert_temp(curve_preview_id, preview));

        if let Some(point) = preview {
            p.circle_filled(point, 5.0, self.theme.panel.marker);
            p.circle_stroke(point, 5.0, Stroke::new(2.0, self.theme.curves.response));
        }

        let multiple_picked = markers
            .iter()
            .filter(|(index, _)| self.is_picked(*index))
            .take(2)
            .count()
            == 2;

        let hovered_marker = plot_hover.and_then(point_at);
        let pressed_marker = self.drag.as_ref().map(|drag| drag.band).or_else(|| {
            press_target
                .filter(|_| {
                    primary_down
                        && response.is_pointer_button_down_on()
                        && !over_panel
                        && !popup_open
                })
                .map(|(index, _)| index)
        });

        for &(index, point) in &markers {
            let target_scale = if pressed_marker == Some(index) {
                0.92
            } else if hovered_marker == Some(index) {
                1.14
            } else {
                1.0
            };
            let scale = ui.ctx().animate_value_with_time(
                response.id.with(("band-marker-scale", index)),
                target_scale,
                0.10,
            );

            let selected = self.selected == Some(index);
            let on = controls[index].enabled;
            let color = self.theme.band[index];
            // dim ghost marker for disabled bands
            let ink = if on {
                color
            } else {
                color.gamma_multiply(0.45)
            };

            if selected {
                p.circle_filled(
                    point,
                    12.0 * scale,
                    color.gamma_multiply(if on { 0.16 } else { 0.08 }),
                );
            }

            if multiple_picked && self.is_picked(index) {
                p.circle_stroke(point, 10.5 * scale, Stroke::new(1.2, Color32::WHITE));
            }

            p.circle_filled(point, 8.0 * scale, self.theme.panel.marker);
            p.circle_stroke(
                point,
                8.0 * scale,
                Stroke::new(if selected { 2.4 } else { 1.4 }, ink),
            );
            p.text(
                point,
                egui::Align2::CENTER_CENTER,
                format!("{}", index + 1),
                FontId::monospace(9.5),
                ink,
            );
        }

        // draw marquee box
        if let Some((start, end)) = self.marquee {
            let band = Rect::from_two_pos(start, end);
            let accent = self.accent();
            p.rect_filled(band, 0.0, accent.gamma_multiply(0.12));
            p.rect_stroke(
                band,
                0.0,
                Stroke::new(1.0, accent.gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            );
        }

        self.readout(&p, &plot, &display_controls);
        self.brand_click(ui, brand_rect);
    }

    fn readout(&self, p: &egui::Painter, plot: &Plot, controls: &[BandControl; BAND_COUNT]) {
        let Some(index) = self.selected else { return };
        let control = controls[index];
        let mut text = format!(
            "BAND {}  {}  {}  ",
            index + 1,
            control.kind.tag(),
            control.channel.tag()
        );
        text.push_str(&format_hz(control.frequency));
        if control.kind.has_gain() {
            text.push_str(&format!("  {:+.1} dB", control.gain));
        }
        if !control.kind.slopes().is_empty() {
            text.push_str(&format!("  {}", control.slope.label()));
            if control.kind.uses_q(control.slope) {
                text.push_str(&format!("  Q {:.3}", control.q));
            }
        }

        p.text(
            plot.rect.min + Vec2::new(10.0, 42.0),
            egui::Align2::LEFT_TOP,
            text,
            FontId::monospace(11.0),
            if control.enabled {
                self.theme.text.primary
            } else {
                self.theme.text.muted.gamma_multiply(0.76)
            },
        );
    }
    fn brand_click(&mut self, ui: &mut egui::Ui, rect: Rect) {
        if self.footer_menu_is_up() || self.drag.is_some() || self.marquee.is_some() {
            return;
        }

        let response = ui
            .interact(rect, ui.make_persistent_id("equart-brand"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        if response.clicked() {
            let now = ui.input(|input| input.time);
            self.settings.release(now);
            self.scale.release(now);
            self.layout.release(now);
            self.preset.release(now);

            if self.theme_open {
                self.theme_open = false;
                self.flush_theme(Some(ui.ctx()));
            }

            self.about.open(now);
        }
    }
}

fn draw_brand(ui: &egui::Ui, p: &egui::Painter, plot: &Plot, interactive: bool) -> Rect {
    let logo_size = 24.0;
    let top_left = plot.rect.min + Vec2::new(10.0, 10.0);
    let font = FontId::new(19.0, egui::FontFamily::Name(GLOBAL_FONT.into()));
    let text_size = p
        .layout_no_wrap(crate::NAME.to_owned(), font.clone(), Color32::WHITE)
        .size();
    let rect = Rect::from_min_size(
        top_left,
        Vec2::new(logo_size + 7.0 + text_size.x, logo_size.max(text_size.y)),
    );

    let hovered = interactive
        && ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| rect.contains(pos));
    let hover =
        ui.ctx()
            .animate_bool_with_time(ui.make_persistent_id("equart-brand-hover"), hovered, 0.16);
    let alpha = 0.34 + 0.28 * hover;
    let ink = theme::current(p.ctx()).text.primary.gamma_multiply(alpha);

    draw_logo(p, top_left, logo_size, ink);
    p.text(
        Pos2::new(top_left.x + logo_size + 7.0, top_left.y + logo_size * 0.5),
        egui::Align2::LEFT_CENTER,
        crate::NAME,
        font,
        ink,
    );

    rect
}
