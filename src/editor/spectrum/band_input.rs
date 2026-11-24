use super::*;

pub(crate) struct BandDrag {
    pub(crate) band: usize,
    pub(crate) grab: Vec2,
    pub(crate) origin: (f32, f32),
    pub(crate) origin_y: f32,
    pub(crate) origin_q: f32,
    pub(super) origin_range: f32,
    pub(crate) followers: Vec<(usize, (f32, f32))>,
    pub(super) vertical: VerticalDrag,
}

#[derive(Clone, Copy)]
pub(super) struct VerticalDrag {
    pub(super) y: f32,
    pub(super) db: f32,
    pub(super) start_range: f32,
}

impl VerticalDrag {
    pub(super) fn update(
        &mut self,
        plot: &Plot,
        y: f32,
        range: f32,
        limits: std::ops::RangeInclusive<f32>,
        fine: bool,
    ) -> f32 {
        const TOP_MARGIN: f32 = 6.0;
        const BOTTOM_MARGIN: f32 = 128.0;
        const RETURN_MARGIN: f32 = 12.0;

        let height = plot.rect.height();
        let dy = self.y - y;
        let speed = if fine { 0.1 } else { 1.0 };

        // use pointer travel so zooming the plot keeps the current value
        let before = self.db;
        let db = (before + dy * range * 2.0 / height * speed).clamp(*limits.start(), *limits.end());
        self.y = y;
        self.db = db;

        let edge = |range: f32, margin: f32| range * (1.0 - 2.0 * (margin / height).min(0.4));
        let returning = (dy > 0.0 && before < 0.0) || (dy < 0.0 && before > 0.0);

        if range > self.start_range
            && returning
            && db >= -edge(self.start_range, BOTTOM_MARGIN + RETURN_MARGIN)
            && db <= edge(self.start_range, TOP_MARGIN + RETURN_MARGIN)
        {
            self.start_range
        } else if (dy > 0.0 && db >= edge(range, TOP_MARGIN))
            || (dy < 0.0 && db <= -edge(range, BOTTOM_MARGIN))
        {
            let wanted = db.abs().min(AXIS_RANGES[AXIS_RANGES.len() - 1] as f32);
            AXIS_RANGES
                .iter()
                .map(|&step| step as f32)
                .find(|&step| step > range && step >= wanted)
                .unwrap_or(range)
        } else {
            range
        }
    }
}

pub(super) fn follow(
    from: (f32, f32),
    to: (f32, f32),
    at_start: (f32, f32),
    lead_has_gain: bool,
) -> (f32, f32) {
    let ratio = if from.0 > 0.0 { to.0 / from.0 } else { 1.0 };
    let gain_delta = to.1 - from.1;
    let gain = if lead_has_gain && from.1 * at_start.1 < 0.0 {
        at_start.1 - gain_delta
    } else {
        at_start.1 + gain_delta
    };

    (at_start.0 * ratio, gain)
}

const CUT_HANDLE_DB: f32 = 3.0103;

pub(crate) fn cut_handle_q(control: &BandControl) -> f32 {
    control.q
}

/// The response of our cut
/// cascade at the cutoff is `20·log10(Q)` dB
pub(crate) fn cut_handle_y(plot: &Plot, q: f32) -> f32 {
    plot.y(cut_handle_db(q))
}

pub(super) fn cut_handle_db(q: f32) -> f32 {
    20.0 * q.max(0.01).log10() + CUT_HANDLE_DB
}

pub(super) fn cut_q(param: &FloatParam, from: f32, db_delta: f32, fine: bool) -> f32 {
    let db = if fine { db_delta * 0.1 } else { db_delta };
    let q = (from * 10.0_f32.powf(db / 20.0)).clamp(Q_MIN, Q_MAX);
    param.preview_plain(param.preview_normalized(q))
}

const AUTO_LOW_CUT_HZ: f32 = 20.0;
const AUTO_HIGH_CUT_HZ: f32 = 15_000.0;

pub(super) fn new_band_kind(hz: f32) -> FilterKind {
    if hz <= AUTO_LOW_CUT_HZ {
        FilterKind::Highpass
    } else if hz >= AUTO_HIGH_CUT_HZ {
        FilterKind::Lowpass
    } else {
        FilterKind::Peak
    }
}

pub(super) fn key_hz(plot: &Plot, x: f32) -> f32 {
    let note = note_at(plot.hz_extended(x)).round();
    note_hz(note).clamp(MIN_HZ, plot.max_hz)
}

impl EquartEditor {
    pub(super) fn empty_plot_menu(
        &mut self,
        ui: &mut egui::Ui,
        hz: f32,
        db: f32,
        controls: &[BandControl; BAND_COUNT],
    ) {
        let can_add = self.gui.is_some() && controls.iter().any(|control| !control.enabled);
        if ui
            .add_enabled(can_add, egui::Button::new("Add Band"))
            .clicked()
        {
            self.place_band(hz, db, controls, ui.input(|input| input.time));
            ui.close();
        }
        let can_add_region = self.gui.is_some()
            && self
                .params
                .regions
                .iter()
                .any(|region| !region.enabled.value());
        if ui
            .add_enabled(can_add_region, egui::Button::new("Add Spectral Region"))
            .clicked()
        {
            self.place_region(hz);
            ui.close();
        }
        if ui.selectable_label(self.frozen(), "Freeze").clicked() {
            self.params
                .analyzer_frozen
                .fetch_xor(true, Ordering::Relaxed);
            ui.close();
        }
        ui.separator();
        let can_delete = self.gui.is_some()
            && ((0..BAND_COUNT).any(|index| self.band_exists(index))
                || self
                    .params
                    .regions
                    .iter()
                    .any(|region| region.enabled.value()));
        if ui
            .add_enabled(can_delete, egui::Button::new("Delete all"))
            .clicked()
        {
            self.delete_all_objects();
            ui.close();
        }
    }

    pub(super) fn place_band(
        &mut self,
        hz: f32,
        db: f32,
        controls: &[BandControl; BAND_COUNT],
        time: f64,
    ) {
        let kind = new_band_kind(hz);
        match controls.iter().position(|control| !control.enabled) {
            Some(index) => {
                if let Some(gui) = &self.gui {
                    let setter = gui.param_setter();
                    let band = &self.params.bands[index];
                    self.set(&setter, &band.kind, kind);
                    self.set(&setter, &band.frequency, hz);
                    self.set(&setter, &band.gain, if kind.has_gain() { db } else { 0.0 });
                    self.set(&setter, &band.slope, CutSlope::Db12);
                    self.set(&setter, &band.enabled, true);

                    let mut baseline = controls[index];
                    baseline.kind = kind;
                    baseline.frequency = hz;
                    baseline.gain = if kind.has_gain() { db } else { 0.0 };
                    baseline.slope = CutSlope::Db12;
                    baseline.enabled = true;
                    self.band_conversion_baselines[index] = Some(baseline);
                    self.params.mark_placed(index);
                }
                self.pick_only(index);
                self.notify(format!("Band {} · {}", index + 1, format_hz(hz)), time);
            }
            None => self.notify("No free bands", time),
        }
    }

    pub(super) fn finish_marquee(&mut self, start: Pos2, end: Pos2, markers: &[(usize, Pos2)]) {
        let band = Rect::from_two_pos(start, end);
        if band.width().max(band.height()) < 4.0 {
            self.clear_selection();
            return;
        }

        let centre = band.center();
        self.selected_region = None;
        self.pending_region_selection = None;
        self.picked.fill(false);
        self.selected = markers
            .iter()
            .copied()
            .filter(|(_, point)| band.contains(*point))
            .map(|(index, point)| {
                self.picked[index] = true;
                (index, point.distance(centre))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index);
    }

    pub(crate) fn placed(&self, index: usize) -> bool {
        if self.params.is_placed(index) {
            return true;
        }

        let band = &self.params.bands[index];
        (band.frequency.value() - band.frequency.default_plain_value()).abs() > 1.0
            || (band.gain.value() - band.gain.default_plain_value()).abs() > 0.05
            || (band.q.value() - band.q.default_plain_value()).abs() > 0.01
            || band.kind.value() != band.kind.default_plain_value()
    }
}
