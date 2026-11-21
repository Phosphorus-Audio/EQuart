use super::*;

impl EquartEditor {
    fn update_spectrum(
        &mut self,
        snapshot: &mut crate::analyzer::capture::DisplaySpectrum,
        rate: f32,
    ) {
        #[cfg(debug_assertions)]
        if std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-signal")).exists()
        {
            let analyzer = self
                .demo
                .get_or_insert_with(|| crate::analyzer::Analyzer::new(48_000.0));
            analyzer.apply_settings(crate::analyzer::AnalyzerSettings::read(&self.params));
            let mut left = [0.0; 800];
            let mut right = [0.0; 800];
            crate::engine::test_signal(&mut left, &mut right);

            for (l, r) in left.iter().zip(right.iter()) {
                analyzer.push([*l, *r]);
            }

            return;
        }

        let enabled = self.params.analyzer.load(Ordering::Relaxed);
        #[cfg(feature = "analyzer-profile")]
        let enabled = crate::analyzer::profile::enabled(enabled);
        snapshot.update(
            &self.analyzer_receiver,
            rate,
            crate::analyzer::AnalyzerSettings::read(&self.params),
            enabled,
        );
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

        let values = self.telemetry.meters();
        let accent = self.accent();
        if accent != self.accent_shown {
            self.accent_shown = accent;
            ui.ctx().style_mut_of(egui::Theme::Dark, |style| {
                style.visuals.selection.bg_fill = accent.gamma_multiply(0.3);
                style.visuals.selection.stroke = Stroke::new(1.0, accent);
            });
        }

        let canvas = ui.max_rect();
        ui.allocate_rect(canvas, Sense::hover());
        let origin = canvas.min.to_vec2();
        let at = |x: f32, y: f32| Pos2::new(x, y) + origin;

        let p = ui.painter_at(canvas);
        gradient_rect(&p, canvas, PLATE_TOP, PLATE_BOTTOM);

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

        // keep the meter under the header and footer
        if self.meter_visible() {
            spectrum::draw_level_meter(
                &p,
                canvas,
                &values,
                self.spectrum_range(),
                self.meter_fade(),
                self.piano_keys_visible(),
            );
        }

        // top header bar
        let header = Rect::from_min_max(at(0.0, 0.0), at(canvas.width(), Layout::HEADER));
        gradient_rect(
            &p,
            header,
            Color32::from_rgba_unmultiplied(56, 56, 56, HEADER_ALPHA),
            Color32::from_rgba_unmultiplied(36, 36, 36, HEADER_ALPHA),
        );
        p.line_segment(
            [at(0.0, 1.0), at(canvas.width(), 1.0)],
            Stroke::new(1.0, Color32::from_gray(92)),
        );
        p.line_segment(
            [
                at(0.0, Layout::HEADER - 1.0),
                at(canvas.width(), Layout::HEADER - 1.0),
            ],
            Stroke::new(1.0, Color32::from_gray(12)),
        );

        let logo_size = 26.0;
        draw_logo(
            &p,
            at(11.0, (Layout::HEADER - logo_size) / 2.0),
            logo_size,
            TEXT,
        );

        let title = FontId::new(22.0, egui::FontFamily::Name(GLOBAL_FONT.into()));
        p.text(
            at(45.0, 23.0),
            egui::Align2::LEFT_CENTER,
            crate::NAME,
            title.clone(),
            Color32::from_black_alpha(136),
        );
        p.text(
            at(44.0, 21.5),
            egui::Align2::LEFT_CENTER,
            crate::NAME,
            title,
            TEXT.gamma_multiply(0.85),
        );

        let footer = Rect::from_min_max(
            at(0.0, canvas.height() - Layout::FOOTER),
            at(canvas.width(), canvas.height()),
        );
        gradient_rect(
            &p,
            footer,
            Color32::from_rgba_unmultiplied(46, 46, 46, BAR_ALPHA),
            Color32::from_rgba_unmultiplied(27, 27, 27, BAR_ALPHA),
        );
        p.line_segment(
            [
                at(0.0, canvas.height() - Layout::FOOTER),
                at(canvas.width(), canvas.height() - Layout::FOOTER),
            ],
            Stroke::new(1.0, Color32::from_gray(76)),
        );

        self.band_panel(ui, canvas);
        if self.selected_region.is_some() {
            self.region_panel(ui, canvas, values.sample_rate);
        }
        self.footer(ui, footer, &values);
        // draw dial last so it sits over header/footer
        self.axis_dial(ui, canvas);
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
            p.text(anchor, egui::Align2::RIGHT_CENTER, label, font, MUTED);
            let rect = Rect::from_min_size(Pos2::new(origin.x, y - 2.0), Vec2::new(width, 4.0));
            p.rect_filled(rect, 0.0, Color32::from_gray(26));
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
                    RED
                } else {
                    Color32::from_gray(155)
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
            TEXT,
        );
        ctx.request_repaint_after(Duration::from_millis(120));
    }
}
