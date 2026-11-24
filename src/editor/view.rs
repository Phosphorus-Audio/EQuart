use super::*;

impl EquartEditor {
    fn update_spectrum(
        &mut self,
        snapshot: &mut crate::analyzer::capture::DisplaySpectrum,
        rate: f32,
    ) {
        let settings = crate::analyzer::AnalyzerSettings::read(
            &self.params,
            self.display_preferences.read().spectrum,
        );

        #[cfg(debug_assertions)]
        if std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-signal")).exists()
        {
            let analyzer = self
                .demo
                .get_or_insert_with(|| crate::analyzer::Analyzer::new(48_000.0));
            analyzer.apply_settings(settings);
            let mut left = [0.0; 800];
            let mut right = [0.0; 800];
            crate::engine::test_signal(&mut left, &mut right);

            for (l, r) in left.iter().zip(right.iter()) {
                analyzer.ingest([*l, *r]);
            }

            analyzer.refresh();
            return;
        }

        let enabled = self.params.analyzer.load(Ordering::Relaxed);
        #[cfg(feature = "analyzer-profile")]
        let enabled = crate::analyzer::profile::enabled(enabled);
        snapshot.update(&self.analyzer_receiver, rate, settings, enabled);
    }

    pub(super) fn restore_ui_scale(&self, ctx: &egui::Context) -> Result<(), std::io::Error> {
        let percent = self.params.ui_scale_percent.load(Ordering::Relaxed);

        if !UI_SCALES.contains(&percent) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid saved UI Scale: {percent}%"),
            ));
        }

        let scale = percent as f32 / 100.0;

        if (ctx.zoom_factor() - scale).abs() > 0.001 {
            // zoom only, layout stretches with window
            ctx.set_zoom_factor(scale);
        }

        Ok(())
    }

    pub(super) fn draw(&mut self, ui: &mut egui::Ui) {
        if let Err(error) = self.restore_ui_scale(ui.ctx()) {
            ui.colored_label(RED, error.to_string());
            return;
        }

        self.reveal_pending_region();
        let values = self.telemetry.meters();
        self.theme = self.theme_for_frame(ui.ctx());
        let accent = self.accent();

        if accent != self.accent_shown || self.theme != theme::current(ui.ctx()) {
            self.accent_shown = accent;
            theme::install(ui.ctx(), self.theme, accent);
            ui.set_style(ui.ctx().style_of(egui::Theme::Dark));
        }

        let palette = self.theme;
        let canvas = ui.max_rect();
        ui.allocate_rect(canvas, Sense::hover());
        let origin = canvas.min.to_vec2();
        let at = |x: f32, y: f32| Pos2::new(x, y) + origin;

        let p = ui.painter_at(canvas);
        let background = palette.background;
        gradient_rect(&p, canvas, background.top, background.bottom);

        // draw plot first so translucent header sits on top
        let mut snapshot = std::mem::take(&mut self.spectrum_snapshot);
        self.update_spectrum(&mut snapshot, values.sample_rate);
        #[cfg(debug_assertions)]
        if self.demo.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        let frame = snapshot.frame();
        #[cfg(debug_assertions)]
        let demo = self.demo.take();
        #[cfg(debug_assertions)]
        let frame = demo.as_ref().map_or(frame, |demo| {
            crate::analyzer::SpectrumFrame::from_analyzers([demo, demo])
        });
        self.spectrum(ui, canvas, values.sample_rate, &frame);
        self.spectrum_snapshot = snapshot;
        #[cfg(debug_assertions)]
        {
            self.demo = demo;
        }

        self.level_meter(ui, &p, canvas, &values);

        let footer = Rect::from_min_max(
            at(0.0, canvas.height() - Layout::FOOTER),
            at(canvas.width(), canvas.height()),
        );
        gradient_rect(
            &p,
            footer,
            theme::with_alpha(background.footer_top, BAR_ALPHA),
            theme::with_alpha(background.footer_bottom, BAR_ALPHA),
        );

        if self.display_preferences.read().layout.footer_border {
            p.line_segment(
                [footer.left_top(), footer.right_top()],
                Stroke::new(1.0, background.footer_border),
            );
        }

        let style = (self.selected.is_some() || self.selected_region.is_some()).then(|| {
            let style = Arc::clone(ui.style());
            let panel = palette.panel;
            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(panel.label);
            visuals.weak_text_color = Some(panel.muted_label);
            visuals.window_fill = panel.parameter;

            if panel.parameter.intensity() > 0.5 {
                visuals.widgets = egui::Visuals::light().widgets;
                visuals.text_edit_bg_color = Some(panel.parameter.lerp_to_gamma(Color32::WHITE, 0.35));
            }

            style
        });

        self.band_panel(ui, canvas);
        if self.selected_region.is_some() {
            self.region_panel(ui, canvas, values.sample_rate);
        }

        if let Some(style) = style {
            ui.set_style(style);
        }

        self.footer(ui, footer, &values);
        self.axis_dial(ui, canvas);
        self.about_dialog(ui.ctx(), canvas);
        self.show_conversion_dialog(ui.ctx());
        self.show_notice(ui.ctx(), canvas);
    }

    pub(super) fn level_meters(
        &self,
        p: &egui::Painter,
        origin: Pos2,
        width: f32,
        values: &MeterReadings,
    ) {
        for (index, (label, peak)) in [("IN", values.input_peak), ("OUT", values.output_peak)]
            .into_iter()
            .enumerate()
        {
            let y = origin.y + index as f32 * 11.0;
            let anchor = Pos2::new(origin.x - 8.0, y);
            let font = FontId::proportional(9.0);
            menu::label_plate_on(p, label, font.clone(), anchor, egui::Align2::RIGHT_CENTER);
            p.text(anchor, egui::Align2::RIGHT_CENTER, label, font, self.theme.text.muted);
            let rect = Rect::from_min_size(Pos2::new(origin.x, y - 2.0), Vec2::new(width, 4.0));
            p.rect_filled(rect, 0.0, self.theme.meter.compact_track);
            let fraction = if peak > 0.0 {
                ((20.0 * peak.log10() + 60.0) / 66.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let fill =
                Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));
            p.rect_filled(
                fill,
                0.0,
                if peak > 1.0 {
                    self.theme.meter.red
                } else {
                    self.theme.meter.compact_level
                },
            );
        }
    }

    fn show_notice(&mut self, ctx: &egui::Context, canvas: Rect) {
        let now = ctx.input(|i| i.time);
        let Some(notice) = &self.notice else { return };

        if now > notice.expires_at {
            self.notice = None;
            return;
        }

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("equart-notice"),
        ));
        let rect = Rect::from_center_size(
            Pos2::new(canvas.center().x, canvas.bottom() - 26.0),
            Vec2::new(260.0, 26.0),
        );
        painter.rect_filled(rect, 4.0, Color32::from_black_alpha(190));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &notice.message,
            FontId::proportional(12.0),
            self.theme.text.primary,
        );
        ctx.request_repaint_after(Duration::from_millis(120));
    }
}
