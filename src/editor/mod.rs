use crate::{
    Telemetry,
    band::{BAND_COUNT, BandControl},
    display_preferences::{DisplayPreferences, DisplayStore, theme::Theme},
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
mod wheel;

mod controls;
mod dial;
mod icons;
mod knob;
mod menu;
mod modifiers;
mod regions;
mod spectrum;
mod theme;

use knob::{RED, gradient_rect};

pub(crate) const GLOBAL_FONT: &str = "global-bold";

pub(crate) use layout::Layout;
use logo::draw_logo;

pub(crate) const CANVAS_W: f32 = 960.0;
pub(crate) const CANVAS_H: f32 = 700.0;
#[cfg(test)]
pub(crate) const CANVAS: Vec2 = Vec2::new(CANVAS_W, CANVAS_H);

const BAR_ALPHA: u8 = 160;
/// header is more transparent than footer
const HEADER_ALPHA: u8 = 112;

/// 60fps cap when idle
const IDLE_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

pub struct EquartEditor {
    params: Arc<EquartParams>,
    display_preferences: Arc<DisplayStore>,
    telemetry: Arc<Telemetry>,
    analyzer_receiver: Arc<crate::analyzer::capture::Receiver>,
    spectrum_snapshot: crate::analyzer::capture::DisplaySpectrum,
    response_db: [Vec<f32>; BAND_COUNT],
    fir_response: crate::telemetry::FirResponse,
    gui: Option<GuiContext>,
    /// focused band for the panel
    pub(crate) selected: Option<usize>,
    selected_region: Option<usize>,
    held_delta: Option<usize>,
    held_audition: Option<crate::engine::audition::Target>,
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
    theme_open: bool,
    pending_theme: Option<theme::PendingTheme>,
    theme_preset_dialog: Option<theme::PresetDialog>,
    theme: Theme,
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
    pub(crate) fn new(
        params: Arc<EquartParams>,
        display_preferences: Arc<DisplayStore>,
        telemetry: Arc<Telemetry>,
        analyzer_receiver: Arc<crate::analyzer::capture::Receiver>,
    ) -> Self {
        let theme = display_preferences.read().theme;

        Self {
            params,
            display_preferences,
            telemetry,
            analyzer_receiver,
            spectrum_snapshot: Default::default(),
            response_db: Default::default(),
            fir_response: Default::default(),
            gui: None,
            selected: None,
            selected_region: None,
            held_delta: None,
            held_audition: None,
            picked: [false; crate::band::BAND_COUNT],
            marquee: None,
            drag: None,
            slope_wheel: 0.0,
            axis_glow: [0.0; 2],
            accent_shown: theme.panel.accent,
            notice: None,
            settings: controls::CardLife::SHUT,
            scale: controls::CardLife::SHUT,
            layout: controls::CardLife::SHUT,
            theme_open: false,
            pending_theme: None,
            theme_preset_dialog: None,
            theme,
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
        if let Some(index) = self.selected_region {
            self.theme.region[index]
        } else {
            self.selected
                .map_or(self.theme.panel.accent, |index| self.theme.band[index])
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

    fn save_display_preferences(
        &mut self,
        ctx: &egui::Context,
        change: impl FnOnce(&mut DisplayPreferences),
    ) {
        if let Err(error) = self.display_preferences.update(change) {
            nice_plug::nice_error!("Could not save EQuart display preferences: {error}");
            self.notify(
                "Could not save display preferences",
                ctx.input(|input| input.time),
            );
        }

        ctx.request_repaint();
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
        self.telemetry.set_audition(None);
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
        self.held_audition = None;
        self.telemetry.set_delta_region(None);
        self.telemetry.set_audition(None);
        self.gui = None;
        self.drag = None;
        self.dial.shut();
        self.theme_open = false;
        self.theme_preset_dialog = None;
        self.flush_theme(None);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut nice_plug_egui::Frame) {
        #[cfg(feature = "analyzer-profile")]
        let _frame = crate::analyzer::profile::Timer::new(crate::analyzer::profile::FRAME);
        self.held_delta = None;
        self.held_audition = None;
        self.draw(ui);

        let audition = self.held_audition.filter(|target| match target {
            crate::engine::audition::Target::Band(index) => self.selected == Some(*index),
            crate::engine::audition::Target::Region(index) => self.selected_region == Some(*index),
        });
        self.telemetry.set_audition(audition);
        self.telemetry.set_delta_region(
            self.held_delta.filter(|&i| audition.is_none() && self.selected_region == Some(i)),
        );
        // full framerate when animating, 60fps when idle
        let animated = self.settings.is_shown()
            || self.layout.is_shown()
            || self.scale.is_shown()
            || self.dial.is_shown()
            || self.drag.is_some()
            || self.marquee.is_some()
            || self.held_delta.is_some()
            || audition.is_some();
        #[cfg(feature = "analyzer-profile")]
        if let Some(fps) = crate::analyzer::profile::fps() {
            ui.ctx().request_repaint_after(Duration::from_secs_f64(1.0 / f64::from(fps)));
            return;
        }

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

    let style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
    let default = Theme::default();
    theme::install(ctx, default, default.panel.accent);
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
