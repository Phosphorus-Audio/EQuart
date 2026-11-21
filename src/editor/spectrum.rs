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
mod keys;
mod meter;
mod panel_motion;
mod phase;
mod plot;
mod response;
mod settings;
mod trace;

use backdrop::{ease_glow, edge_fade, soft_wash};
pub(crate) use band_input::{BandDrag, cut_handle_q, cut_handle_y};
use band_input::{cut_q, follow, key_hz, take_steps, wheel_steps};
use fill::gradient_fill;
use grid::{draw_axis_labels, draw_grid_lines};
use keys::{draw_keys, note_at, note_hz};
pub(crate) use plot::Plot;
use response::{ResponseInput, draw_response};
use trace::draw_spectrum;

pub(crate) use keys::keys_strip;
pub(super) use meter::draw_level_meter;

/// keyboard strip height + top gap
pub(crate) const KEYS_BAND: f32 = 26.0;
const KEYS_HEIGHT: f32 = 22.0;
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
        spectrum: &SpectrumFrame,
    ) {
        let outer = Layout::plot_outer(canvas, self.meter_visible());
        let p = ui.painter_at(outer);

        let plot = Plot::with_ranges(outer, rate, self.axis_range(), self.spectrum_range());
        // hide hover note readout when dB dial is open
        let hover = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|_| !self.dial.is_shown());
        let (eq_highlight, spectrum_highlight) = self.axis_controls(ui, &plot, outer);
        draw_grid_lines(&p, &plot, outer);

        if self.params.bypass.value() {
            // fade edges before watermark
            edge_fade(&p, outer, canvas, plot.rect);
            draw_axis_labels(
                &p,
                &plot,
                outer,
                hover,
                eq_highlight,
                spectrum_highlight,
                self.accent(),
            );
            p.text(
                plot.rect.center(),
                egui::Align2::CENTER_CENTER,
                "BYPASS",
                FontId::new(80.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
                Color32::from_white_alpha(36),
            );
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
        let pressed_band_id = response.id.with("pressed-tracked-band");
        if ui.input(|i| i.pointer.primary_pressed()) {
            let tracked = pressed_at.and_then(point_at).filter(|&i| {
                response.is_pointer_button_down_on()
                    && controls[i].enabled
                    && controls[i].pitch.enabled
            });
            ui.data_mut(|data| data.insert_temp(pressed_band_id, tracked));
        }
        // keep tracked band from press pos in case it moved
        let pressed_band = ui
            .data(|data| data.get_temp::<Option<usize>>(pressed_band_id))
            .flatten();
        let clicked_band = pressed_band.or(nearest);

        if response.clicked()
            && let Some(index) = clicked_band
        {
            self.pick_only(index);
        }

        // don't treat clicks on panel or popups as blank plot clicks
        let panel = self
            .region_panel_rect(canvas, rate)
            .or_else(|| self.selected.map(|_| self.band_panel_rect(ui, canvas)));
        let over_panel =
            pointer.is_some_and(|position| panel.is_some_and(|panel| panel.contains(position)));
        let popup_open = egui::Popup::is_any_open(ui.ctx())
            || self.band_dropdown_is_up()
            || self.footer_menu_is_up()
            || self.dial.is_shown();
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
                        self.drag = Some(BandDrag {
                            band: index,
                            grab,
                            origin: (controls[index].frequency, controls[index].gain),
                            origin_y: position.y + grab.y,
                            origin_q: controls[index].q,
                            followers,
                        });
                    }
                }
                // start marquee box on empty space
                None if !over_panel => {
                    if let Some(start) = pressed_at.or(pointer) {
                        self.marquee = Some((start, start));
                        self.drag = None;
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
                && let Some(position) = ui.input(|i| i.pointer.interact_pos())
                && let Some(gui) = &self.gui
            {
                let setter = gui.param_setter();
                let position = position + drag.grab;
                let band = &self.params.bands[drag.band];
                let hz = if band.pitch.enabled.value() {
                    drag.origin.0
                } else {
                    let hz = plot.hz(position.x);
                    self.set(&setter, &band.frequency, hz);
                    hz
                };
                let db = plot.db(position.y);
                if controls[drag.band].kind.has_gain() {
                    self.set(&setter, &band.gain, db);
                } else if controls[drag.band].kind.is_cut()
                    && controls[drag.band].slope != CutSlope::Db6
                {
                    // cuts use vertical drag for Q (except 6dB/oct)
                    let fine = ui.input(|i| i.modifiers.shift);
                    let db_delta = plot.db_free(position.y) - plot.db_free(drag.origin_y);
                    let q = cut_q(&band.q, drag.origin_q, db_delta, fine);
                    self.set(&setter, &band.q, q);
                }

                // move other picked bands together
                for &(index, at_start) in &drag.followers {
                    let (moved_hz, moved_db) = follow(drag.origin, (hz, db), at_start);
                    let follower = self.params.band(index);
                    if !follower.pitch.enabled.value() {
                        self.set(&setter, &follower.frequency, moved_hz);
                    }
                    if self.control(index).kind.has_gain() {
                        self.set(&setter, &self.params.bands[index].gain, moved_db);
                    }
                }
            }
        }

        if response.drag_stopped_by(egui::PointerButton::Primary) {
            self.drag = None;
            if let Some((start, end)) = self.marquee.take() {
                self.finish_marquee(start, end, &markers);
            }
        }

        // wheel edits Q on bells or slope on cuts (uses hover_pos since pointer isn't down)
        let wheel_nearest = response.hover_pos().and_then(point_at);
        let mut q_change = None;
        let mut slope_change = None;
        if let Some(index) = wheel_nearest {
            if controls[index].kind.is_cut() {
                let travel = ui.input(|i| wheel_steps(&i.raw.events));
                let (steps, leftover) = take_steps(self.slope_wheel + travel);
                self.slope_wheel = leftover;
                if steps != 0 {
                    slope_change = Some((index, controls[index].slope.step(steps)));
                }
            } else {
                self.slope_wheel = 0.0;
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 && controls[index].kind.uses_q(controls[index].slope) {
                    let fine = if ui.input(|i| i.modifiers.shift) {
                        0.2
                    } else {
                        1.0
                    };
                    let q =
                        (controls[index].q * (1.0 + scroll * 0.0025 * fine)).clamp(Q_MIN, Q_MAX);
                    q_change = Some((index, q));
                }
            }
        } else {
            // reset leftover trackpad wheel delta when leaving node
            self.slope_wheel = 0.0;
        }
        if let Some((index, q)) = q_change {
            if let Some(gui) = &self.gui {
                let setter = gui.param_setter();
                self.set(&setter, &self.params.bands[index].q, q);
            }
            self.selected_region = None;
            self.selected = Some(index);
        }
        if let Some((index, slope)) = slope_change {
            if let Some(gui) = &self.gui {
                let setter = gui.param_setter();
                self.set(&setter, &self.params.bands[index].slope, slope);
            }
            self.selected_region = None;
            self.selected = Some(index);
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
        let processing = self.params.processing_mode.value();
        let oversampling = processing.oversampling(self.params.oversampling.value());
        let design_rate = if rate > 0.0 { rate } else { 48_000.0 } * oversampling.factor() as f32;
        let phase_mode = self.params.phase_curve.load(Ordering::Relaxed).then(|| {
            if self.params.phase_unwrapped.load(Ordering::Relaxed) {
                phase::Mode::Unwrapped
            } else {
                phase::Mode::Wrapped
            }
        });
        let linear_phase = processing == crate::processing_mode::ProcessingMode::LinearPhase;
        if linear_phase {
            self.fir_response.update(&self.telemetry.fir);
        }
        draw_response(
            &p,
            &plot,
            outer,
            &mut self.response_db,
            ResponseInput {
                controls: &display_controls,
                offsets: &self.telemetry.dynamics(),
                regions: &regions,
                telemetry: &self.telemetry,
                rate: design_rate,
                selected: self.selected,
                phase_mode,
                linear_center: linear_phase.then(|| self.params.linear_quality.value().center_at(design_rate)),
                fir_response: &self.fir_response,
            },
        );

        // fade plot edges before drawing labels
        edge_fade(&p, outer, canvas, plot.rect);
        draw_axis_labels(
            &p,
            &plot,
            outer,
            hover,
            eq_highlight,
            spectrum_highlight,
            self.accent(),
        );
        if let Some(mode) = phase_mode {
            phase::draw_label(&p, &plot, outer, mode);
        }

        for &(index, point) in &markers {
            let selected = self.selected == Some(index);
            let on = controls[index].enabled;
            let color = POINT_COLORS[index];
            // dim ghost marker for disabled bands
            let ink = if on {
                color
            } else {
                color.gamma_multiply(0.45)
            };

            if selected {
                p.circle_filled(
                    point,
                    12.0,
                    color.gamma_multiply(if on { 0.16 } else { 0.08 }),
                );
            } else if self.is_picked(index) {
                // thin ring for other multi-selected bands
                p.circle_stroke(point, 10.5, Stroke::new(1.2, ink.gamma_multiply(0.5)));
            }
            p.circle_filled(point, 8.0, Color32::from_gray(27));
            p.circle_stroke(
                point,
                8.0,
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
            text.push_str(&format!("  {}  Q {:.3}", control.slope.label(), control.q));
        }

        p.text(
            plot.rect.min + Vec2::new(6.0, 10.0),
            egui::Align2::LEFT_TOP,
            text,
            FontId::monospace(11.0),
            if control.enabled {
                TEXT
            } else {
                Color32::from_gray(110)
            },
        );
    }
}
