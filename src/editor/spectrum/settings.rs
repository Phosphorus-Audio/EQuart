use super::*;

impl EquartEditor {
    /// current EQ y scale, fallback to default if invalid
    pub(in crate::editor) fn axis_range(&self) -> f32 {
        let value = self.params.axis_range_db.load(Ordering::Relaxed);
        if AXIS_RANGES.contains(&value) {
            value as f32
        } else {
            DEFAULT_AXIS_RANGE as f32
        }
    }

    /// current spectrum y range
    pub(in crate::editor) fn spectrum_range(&self) -> f32 {
        crate::analyzer::spectrum_range(self.params.spectrum_range_db.load(Ordering::Relaxed))
            as f32
    }

    /// fade out bottom of the level meter
    pub(in crate::editor) fn meter_fade(&self) -> bool {
        self.display_preferences.read().layout.meter_fade
    }

    pub(in crate::editor) fn meter_visible(&self) -> bool {
        self.display_preferences.read().layout.meter_visible
    }

    pub(in crate::editor) fn piano_keys_visible(&self) -> bool {
        self.display_preferences.read().layout.piano_keys_visible
    }

    /// analyzer resolution index
    pub(in crate::editor) fn resolution(&self) -> u32 {
        self.display_preferences
            .read()
            .spectrum
            .index(AnalyzerOption::Resolution)
    }

    /// analyzer speed index
    pub(in crate::editor) fn speed(&self) -> u32 {
        self.display_preferences
            .read()
            .spectrum
            .index(AnalyzerOption::Speed)
    }

    /// spectrum smoothing index
    pub(in crate::editor) fn smoothing(&self) -> u32 {
        self.display_preferences
            .read()
            .spectrum
            .index(AnalyzerOption::Smoothing)
    }

    pub(super) fn hold_index(&self) -> u32 {
        self.display_preferences
            .read()
            .spectrum
            .index(AnalyzerOption::Hold)
    }

    pub(super) fn hold(&self) -> Hold {
        Hold::from_index(self.hold_index())
    }

    /// spectrum tilt index
    pub(in crate::editor) fn tilt(&self) -> u32 {
        self.display_preferences
            .read()
            .spectrum
            .index(AnalyzerOption::Tilt)
    }

    pub(in crate::editor) fn frozen(&self) -> bool {
        self.params.analyzer_frozen.load(Ordering::Relaxed)
    }

    /// left/right axis hover zones, returns highlight alphas for label pass
    pub(super) fn axis_controls(
        &mut self,
        ui: &mut egui::Ui,
        plot: &Plot,
        outer: Rect,
    ) -> (f32, f32) {
        let left = Rect::from_min_max(
            Pos2::new(outer.left(), outer.top()),
            Pos2::new(plot.rect.left(), plot.rect.bottom()),
        );
        let right = Rect::from_min_max(
            Pos2::new(plot.rect.right(), outer.top()),
            Pos2::new(outer.right(), plot.rect.bottom()),
        );
        let eq = self.axis_picker(ui, left, "eq-axis-scale", AxisScale::Eq, 0);
        let spectrum = self.axis_picker(ui, right, "spectrum-axis-scale", AxisScale::Spectrum, 1);
        (eq, spectrum)
    }

    fn axis_picker(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        id: &'static str,
        scale: AxisScale,
        slot: usize,
    ) -> f32 {
        let response = ui
            .interact(rect, ui.make_persistent_id(id), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        let target = if response.hovered() {
            if response.is_pointer_button_down_on() {
                AXIS_PRESS_ALPHA
            } else {
                AXIS_HOVER_ALPHA
            }
        } else {
            0.0
        };
        // smooth hover/press glow
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, 0.1);
        let glow = ease_glow(self.axis_glow[slot], target, dt);
        self.axis_glow[slot] = glow;
        if glow != target {
            ui.ctx().request_repaint();
        }

        // open fan menu towards inside of window
        if response.clicked() && (!self.dial.is_up() || self.dial_axis != scale) {
            let at = ui
                .input(|i| i.pointer.interact_pos())
                .unwrap_or_else(|| rect.center());
            let side = match scale {
                AxisScale::Eq => dial::Side::Right,
                AxisScale::Spectrum => dial::Side::Left,
            };
            self.dial.open(at, ui.input(|i| i.time), side);
            self.dial_axis = scale;
        }

        self.axis_glow[slot]
    }

    /// dB range fan menu shared by both EQ and spectrum axes
    pub(in crate::editor) fn axis_dial(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        if !self.dial.is_shown() {
            return;
        }
        let (options, current, unit): (&[u32], u32, &str) = match self.dial_axis {
            AxisScale::Eq => (&AXIS_RANGES, self.axis_range() as u32, "±"),
            AxisScale::Spectrum => (&SPECTRUM_RANGES, self.spectrum_range() as u32, ""),
        };
        let current = options.iter().position(|&range| range == current);
        let labels: Vec<String> = options
            .iter()
            .map(|range| format!("{unit}{range}"))
            .collect();
        if let dial::Outcome::Picked(index) = self.dial.show(ui, canvas, &labels, current)
            && let Some(&range) = options.get(index)
        {
            match self.dial_axis {
                AxisScale::Eq => self.params.axis_range_db.store(range, Ordering::Relaxed),
                AxisScale::Spectrum => self
                    .params
                    .spectrum_range_db
                    .store(range, Ordering::Relaxed),
            }
        }
    }

    /// analyzer toggles and dropdowns, shared by axis menu and settings card
    pub(in crate::editor) fn analysis_rows(
        &self,
        ui: &mut egui::Ui,
        dropdowns: &mut menu::Dropdown,
        group: egui::Id,
    ) {
        let accent = self.accent();

        menu::section(ui, "ANALYSER");
        let analyzer = self.params.analyzer.load(Ordering::Relaxed);
        if menu::switch(ui, group.with("analyzer"), "Analyzer", analyzer, accent) {
            self.params.analyzer.store(!analyzer, Ordering::Relaxed);
        }
        // freeze keeps max peak
        let frozen = self.frozen();
        if menu::switch(ui, group.with("freeze"), "Freeze", frozen, accent) {
            self.params
                .analyzer_frozen
                .fetch_xor(true, Ordering::Relaxed);
        }

        menu::divider(ui);
        menu::section(ui, "SPECTRUM");
        let resolution = self.resolution() as usize;
        dropdowns.combo(
            ui,
            menu::COMBO_RESOLUTION,
            "Resolution",
            RESOLUTION_LABELS[resolution],
            &RESOLUTION_LABELS,
            resolution,
            accent,
        );
        let speed = self.speed() as usize;
        dropdowns.combo(
            ui,
            menu::COMBO_SPEED,
            "Speed",
            SPECTRUM_SPEED_LABELS[speed],
            &SPECTRUM_SPEED_LABELS,
            speed,
            accent,
        );
        let smoothing = self.smoothing() as usize;
        dropdowns.combo(
            ui,
            menu::COMBO_SMOOTHING,
            "Smoothing",
            SPECTRUM_SMOOTHINGS[smoothing],
            &SPECTRUM_SMOOTHINGS,
            smoothing,
            accent,
        );
        // tilt adjusts visual slope (e.g. 4.5 dB/oct), doesn't change audio
        let tilt = self.tilt() as usize;
        dropdowns.combo(
            ui,
            menu::COMBO_TILT,
            "Tilt",
            TILT_LABELS[tilt],
            &TILT_LABELS,
            tilt,
            accent,
        );
        let hold_index = self.hold_index() as usize;
        dropdowns.combo(
            ui,
            menu::COMBO_HOLD,
            "Peak hold",
            HOLD_MODES[hold_index],
            &HOLD_MODES,
            hold_index,
            accent,
        );

        let frame_rate = self
            .display_preferences
            .read()
            .spectrum
            .index(AnalyzerOption::FrameRate) as usize;
        let labels = &crate::analyzer::FRAME_RATE_LABELS;
        dropdowns.combo(
            ui,
            menu::COMBO_FRAME_RATE,
            "Frame rate",
            labels[frame_rate],
            labels,
            frame_rate,
            accent,
        );
    }

    pub(in crate::editor) fn phase_rows(
        &mut self,
        ui: &mut egui::Ui,
        dropdowns: &mut menu::Dropdown,
        group: egui::Id,
    ) {
        let accent = self.accent();
        let layout = self.display_preferences.read().layout;

        if menu::switch(
            ui,
            group.with("phase-curve"),
            "Phase curve",
            layout.phase_curve,
            accent,
        ) {
            self.save_display_preferences(ui.ctx(), |prefs| {
                prefs.layout.phase_curve = !layout.phase_curve;
            });
        }

        dropdowns.new_group();
        let index = usize::from(layout.phase_unwrapped);
        dropdowns.combo(
            ui,
            menu::COMBO_PHASE,
            "Phase mode",
            phase::Mode::LABELS[index],
            &phase::Mode::LABELS,
            index,
            accent,
        );
    }

    pub(in crate::editor) fn apply_analysis_choice(
        &mut self,
        ctx: &egui::Context,
        key: u32,
        index: usize,
    ) {
        self.save_display_preferences(ctx, |prefs| match key {
            menu::COMBO_RESOLUTION => prefs.spectrum.resolution = index as u32,
            menu::COMBO_SPEED => prefs.spectrum.speed = index as u32,
            menu::COMBO_SMOOTHING => prefs.spectrum.smoothing = index as u32,
            menu::COMBO_TILT => prefs.spectrum.tilt = index as u32,
            menu::COMBO_HOLD => prefs.spectrum.peak_hold = index as u32,
            menu::COMBO_FRAME_RATE => prefs.spectrum.frame_rate = index as u32,
            menu::COMBO_PHASE => prefs.layout.phase_unwrapped = index == 1,
            _ => {}
        });
    }
}
