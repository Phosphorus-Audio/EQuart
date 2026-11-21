use crate::{
    Telemetry,
    band::{BAND_COUNT, BandControl},
    params::{
        AXIS_RANGES, DEFAULT_AXIS_RANGE, DEFAULT_SPECTRUM_RANGE, EquartParams, SPECTRUM_RANGES,
        UI_SCALES,
    },
    telemetry::MeterReadings,
};
use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Vec2};
use nice_plug::{
    context::gui::{GuiContext, ParamSetter},
    prelude::*,
};
use nice_plug_egui::NiceEguiApp;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

mod bands;
mod layout;
mod logo;
mod view;

mod controls;
mod dial;
mod icons;
mod knob;
mod menu;
mod modifiers;
mod regions;
mod spectrum;

use knob::{RED, gradient_rect};

pub(crate) const TEXT: Color32 = Color32::from_rgb(213, 214, 214);
pub(crate) const MUTED: Color32 = Color32::from_rgb(145, 148, 150);
pub(crate) const GRID: Color32 = Color32::from_rgb(54, 57, 59);
/// unlabeled grid lines
pub(crate) const TICK: Color32 = Color32::from_gray(34);
/// summed response curve color
pub(crate) const COMBINED: Color32 = Color32::from_rgb(245, 208, 76);
pub(crate) const GLOBAL_FONT: &str = "global-bold";

pub(crate) use layout::Layout;
use logo::draw_logo;

pub(crate) const CANVAS_W: f32 = 960.0;
pub(crate) const CANVAS_H: f32 = 700.0;
#[cfg(test)]
pub(crate) const CANVAS: Vec2 = Vec2::new(CANVAS_W, CANVAS_H);

/// background gradient
pub(crate) const PLATE_TOP: Color32 = Color32::from_gray(24);
pub(crate) const PLATE_BOTTOM: Color32 = Color32::from_gray(32);
const BAR_ALPHA: u8 = 160;
/// header is more transparent than footer
const HEADER_ALPHA: u8 = 112;

/// 60fps cap when idle
const IDLE_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// accent color when nothing is selected
pub(crate) const NEUTRAL: Color32 = Color32::from_rgb(150, 157, 164);

/// first 8 use palette, rest use golden-ratio hue
const fn point_color(i: usize) -> Color32 {
    const PALETTE: [Color32; 8] = [
        RED,
        Color32::from_rgb(196, 130, 45),
        Color32::from_rgb(49, 153, 120),
        Color32::from_rgb(80, 131, 200),
        Color32::from_rgb(151, 108, 192),
        Color32::from_rgb(203, 105, 151),
        Color32::from_rgb(140, 159, 61),
        Color32::from_rgb(50, 164, 182),
    ];

    if i < PALETTE.len() {
        return PALETTE[i];
    }

    // const hsv->rgb without closures
    let s = 0.62_f32;
    let v = 0.82_f32;
    let h = ((i as f32) * 0.618_034) % 1.0 * 6.0;

    let rgb = [
        {
            let k = (h + 5.0) % 6.0;
            let t = if k < 1.0 {
                k
            } else if k < 3.0 {
                1.0
            } else if k < 4.0 {
                4.0 - k
            } else {
                0.0
            };
            v - v * s * t
        },
        {
            let k = (h + 3.0) % 6.0;
            let t = if k < 1.0 {
                k
            } else if k < 3.0 {
                1.0
            } else if k < 4.0 {
                4.0 - k
            } else {
                0.0
            };
            v - v * s * t
        },
        {
            let k = (h + 1.0) % 6.0;
            let t = if k < 1.0 {
                k
            } else if k < 3.0 {
                1.0
            } else if k < 4.0 {
                4.0 - k
            } else {
                0.0
            };
            v - v * s * t
        },
    ];

    Color32::from_rgb(
        (rgb[0] * 255.0) as u8,
        (rgb[1] * 255.0) as u8,
        (rgb[2] * 255.0) as u8,
    )
}

pub(crate) const POINT_COLORS: [Color32; BAND_COUNT] = {
    let mut colors = [Color32::BLACK; BAND_COUNT];
    let mut i = 0;
    while i < BAND_COUNT {
        colors[i] = point_color(i);
        i += 1;
    }
    colors
};

pub struct EquartEditor {
    params: Arc<EquartParams>,
    telemetry: Arc<Telemetry>,
    spectrum_snapshot: crate::telemetry::SpectrumSnapshot,
    response_db: [Vec<f32>; BAND_COUNT],
    fir_response: crate::telemetry::FirResponse,
    gui: Option<GuiContext>,
    /// focused band for the panel
    pub(crate) selected: Option<usize>,
    selected_region: Option<usize>,
    held_delta: Option<usize>,
    /// multi-selected bands (for dragging together)
    pub(crate) picked: [bool; crate::band::BAND_COUNT],
    /// selection box (start, current)
    pub(crate) marquee: Option<(Pos2, Pos2)>,
    pub(crate) drag: Option<spectrum::BandDrag>,
    /// leftover wheel delta for trackpad slope steps
    pub(crate) slope_wheel: f32,
    /// hover alpha for left/right dB axes
    pub(crate) axis_glow: [f32; 2],
    /// cache accent so we don't rebuild egui style every frame
    accent_shown: Color32,
    notice: Option<Notice>,
    settings: controls::CardLife,
    scale: controls::CardLife,
    layout: controls::CardLife,
    band_dropdowns: [controls::CardLife; 3],
    band_dropdown_owner: Option<usize>,
    modifier_picker: Option<modifiers::Picker>,
    reveal_modifier: Option<(usize, modifiers::Modifier)>,
    /// dB range fan menu and which axis opened it
    dial: dial::Dial,
    dial_axis: spectrum::AxisScale,
    /// debug-only analyzer for test signal
    #[cfg(debug_assertions)]
    demo: Option<crate::analyzer::Analyzer>,
}

struct Notice {
    message: String,
    expires_at: f64,
}

impl EquartEditor {
    pub(crate) fn new(params: Arc<EquartParams>, telemetry: Arc<Telemetry>) -> Self {
        Self {
            params,
            telemetry,
            spectrum_snapshot: Default::default(),
            response_db: Default::default(),
            fir_response: Default::default(),
            gui: None,
            selected: None,
            selected_region: None,
            held_delta: None,
            picked: [false; crate::band::BAND_COUNT],
            marquee: None,
            drag: None,
            slope_wheel: 0.0,
            axis_glow: [0.0; 2],
            accent_shown: NEUTRAL,
            notice: None,
            settings: controls::CardLife::SHUT,
            scale: controls::CardLife::SHUT,
            layout: controls::CardLife::SHUT,
            band_dropdowns: [controls::CardLife::SHUT; 3],
            band_dropdown_owner: None,
            modifier_picker: None,
            reveal_modifier: None,
            dial: dial::Dial::SHUT,
            dial_axis: spectrum::AxisScale::Eq,
            #[cfg(debug_assertions)]
            demo: None,
        }
    }

    pub(crate) fn accent(&self) -> Color32 {
        if self.selected_region.is_some() {
            regions::REGION_COLOR
        } else {
            self.selected.map_or(NEUTRAL, |index| POINT_COLORS[index])
        }
    }

    pub(crate) fn control(&self, index: usize) -> BandControl {
        self.params.control(index)
    }

    pub(crate) fn notify(&mut self, message: impl Into<String>, now: f64) {
        self.notice = Some(Notice {
            message: message.into(),
            expires_at: now + 2.5,
        });
    }

    /// Apply a parameter value as one host gesture.
    pub(crate) fn set<P: Param>(&self, setter: &ParamSetter<'_>, param: &P, value: P::Plain) {
        setter.begin_set_parameter(param);
        setter.set_parameter(param, value);
        setter.end_set_parameter(param);
    }
}

impl Drop for EquartEditor {
    fn drop(&mut self) {
        self.telemetry.set_delta_region(None);
        self.drag = None;
    }
}

impl NiceEguiApp for EquartEditor {
    fn build(
        &mut self,
        ctx: egui::Context,
        gui: GuiContext,
        _: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        configure_style(&ctx);
        let _ = self.restore_ui_scale(&ctx);
        self.gui = Some(gui);
        Ok(())
    }

    fn editor_closed(&mut self) {
        self.held_delta = None;
        self.telemetry.set_delta_region(None);
        self.gui = None;
        self.drag = None;
        self.dial.shut();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut nice_plug_egui::Frame) {
        self.held_delta = None;
        self.draw(ui);
        self.telemetry.set_delta_region(self.held_delta.filter(|&i| self.selected_region == Some(i)));
        // full framerate when animating, 60fps when idle
        let animated = self.settings.is_shown()
            || self.layout.is_shown()
            || self.scale.is_shown()
            || self.dial.is_shown()
            || self.drag.is_some()
            || self.marquee.is_some()
            || self.held_delta.is_some();
        if animated {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(IDLE_FRAME_INTERVAL);
        }
    }
}

pub(crate) fn format_hz(hz: f32) -> String {
    if hz >= 1_000.0 {
        format!("{:.2}k", hz / 1_000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

pub(crate) fn configure_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        GLOBAL_FONT.to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/Ubuntu-Bold.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name(GLOBAL_FONT.into()),
        vec![GLOBAL_FONT.to_owned()],
    );
    ctx.set_fonts(fonts);

    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    style.visuals.panel_fill = Color32::from_gray(32);
    style.visuals.override_text_color = Some(TEXT);
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

fn lighten(color: Color32, amount: f32) -> Color32 {
    let channel = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * amount) as u8;
    Color32::from_rgb(channel(color.r()), channel(color.g()), channel(color.b()))
}

fn hover_progress(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    let t = ctx.animate_bool_with_time(id.with("hover-fade"), hovered, 0.16);
    // smoothstep both ways
    t * t * (3.0 - 2.0 * t)
}

fn hover_amount(response: &Response) -> f32 {
    hover_progress(&response.ctx, response.id, response.hovered())
}

#[cfg(test)]
mod tests;
