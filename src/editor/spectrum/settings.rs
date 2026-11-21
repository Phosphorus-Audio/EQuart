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
        self.params.meter_fade.load(Ordering::Relaxed)
    }

    pub(in crate::editor) fn meter_visible(&self) -> bool {
        self.params.meter_visible.load(Ordering::Relaxed)
    }

    pub(in crate::editor) fn piano_keys_visible(&self) -> bool {
        self.params.piano_keys_visible.load(Ordering::Relaxed)
    }

    /// analyzer resolution index
    pub(in crate::editor) fn resolution(&self) -> u32 {
        self.params.analyzer_index(AnalyzerOption::Resolution)
    }

    /// analyzer speed index
    pub(in crate::editor) fn speed(&self) -> u32 {
        self.params.analyzer_index(AnalyzerOption::Speed)
    }

    /// spectrum smoothing index
    pub(in crate::editor) fn smoothing(&self) -> u32 {
        self.params.analyzer_index(AnalyzerOption::Smoothing)
    }

    pub(super) fn hold_index(&self) -> u32 {
        self.params.analyzer_index(AnalyzerOption::Hold)
    }

    pub(super) fn hold(&self) -> Hold {
        Hold::from_index(self.hold_index())
    }

    /// spectrum tilt index
    pub(in crate::editor) fn tilt(&self) -> u32 {
        self.params.analyzer_index(AnalyzerOption::Tilt)
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
            Pos2::new(outer.left(), outer.top() + Layout::HEADER),
            Pos2::new(plot.rect.left(), plot.rect.bottom()),
        );
        let right = Rect::from_min_max(
            Pos2::new(plot.rect.right(), outer.top() + Layout::HEADER),
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
    }

    pub(in crate::editor) fn phase_rows(
        &self,
        ui: &mut egui::Ui,
        dropdowns: &mut menu::Dropdown,
        group: egui::Id,
    ) {
        let accent = self.accent();
        let phase = self.params.phase_curve.load(Ordering::Relaxed);
        if menu::switch(ui, group.with("phase-curve"), "Phase curve", phase, accent) {
            self.params.phase_curve.store(!phase, Ordering::Relaxed);
            ui.ctx().request_repaint();
        }
        dropdowns.new_group();
        let index = usize::from(self.params.phase_unwrapped.load(Ordering::Relaxed));
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

    /// save dropdown choice to params
    pub(in crate::editor) fn apply_analysis_choice(&self, key: u32, index: usize) {
        match key {
            menu::COMBO_RESOLUTION => self
                .params
                .analyzer_resolution
                .store(index as u32, Ordering::Relaxed),
            menu::COMBO_SPEED => self
                .params
                .analyzer_speed
                .store(index as u32, Ordering::Relaxed),
            menu::COMBO_SMOOTHING => self
                .params
                .analyzer_smoothing
                .store(index as u32, Ordering::Relaxed),
            menu::COMBO_TILT => self
                .params
                .analyzer_tilt
                .store(index as u32, Ordering::Relaxed),
            menu::COMBO_HOLD => self
                .params
                .analyzer_hold
                .store(index as u32, Ordering::Relaxed),
            menu::COMBO_PHASE => self
                .params
                .phase_unwrapped
                .store(index == 1, Ordering::Relaxed),
            _ => {}
        }
    }
}
