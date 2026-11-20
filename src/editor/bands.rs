use super::*;
use crate::pitch::{PitchControl, PitchDetectionParams};

impl EquartEditor {
    /// Make one band the whole selection, panel and all.
    pub(crate) fn pick_only(&mut self, index: usize) {
        self.selected_region = None;
        self.picked = [false; crate::band::BAND_COUNT];
        self.picked[index] = true;
        self.selected = Some(index);
    }

    pub(crate) fn clear_selection(&mut self) {
        self.picked = [false; crate::band::BAND_COUNT];
        self.selected = None;
        self.selected_region = None;
        self.drag = None;
        self.slope_wheel = 0.0;
        self.modifier_picker = None;
    }

    /// Check if a band is selected. The focused band counts even if `picked` is empty.
    pub(crate) fn is_picked(&self, index: usize) -> bool {
        self.picked[index] || self.selected == Some(index)
    }

    /// Check if a slot has a point on the plot—either it's on, or it was placed before and stays as a ghost when off.
    pub(crate) fn band_exists(&self, index: usize) -> bool {
        self.control(index).enabled || self.placed(index)
    }

    /// Turn a band on or off. Enabling an untouched slot marks it as placed so it keeps its ghost even if turned off later.
    pub(crate) fn set_band_enabled(&self, index: usize, on: bool) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        if on {
            self.params.mark_placed(index);
        }
        self.set(&setter, &self.params.bands[index].enabled, on);
    }

    pub(super) fn delete_band(&mut self, index: usize) {
        let Some(gui) = &self.gui else { return };
        self.reset_band(&gui.param_setter(), index);
        self.clear_selection();
    }

    pub(super) fn delete_all_objects(&mut self) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        for index in 0..BAND_COUNT {
            if self.band_exists(index) {
                self.reset_band(&setter, index);
            }
        }
        for (index, region) in self.params.regions.iter().enumerate() {
            if region.enabled.value() {
                self.reset_region(&setter, index);
            }
        }
        self.clear_selection();
    }

    pub(super) fn reset_band(&self, setter: &ParamSetter<'_>, index: usize) {
        let band = self.params.band(index);

        self.set(setter, &band.enabled, false);
        self.set(setter, &band.kind, band.kind.default_plain_value());
        self.set(setter, &band.channel, band.channel.default_plain_value());
        self.set(setter, &band.slope, band.slope.default_plain_value());
        self.set(setter, &band.dynamics.enabled, false);
        self.set(setter, &band.dynamics.bypassed, false);
        self.set(setter, &band.dynamics.above_enabled, true);
        self.set(setter, &band.dynamics.below_enabled, false);
        self.set(setter, &band.pitch.enabled, false);
        self.set(setter, &band.pitch.bypassed, false);
        self.write_pitch_detection(setter, &band.pitch.detection, PitchControl::default());
        for detector in [&band.dynamics.detect, &band.dynamics.relative] {
            self.set(setter, &detector.mode, detector.mode.default_plain_value());
            self.set(
                setter,
                &detector.input,
                detector.input.default_plain_value(),
            );
        }
        // Reset values to default so the deleted band isn't mistaken for a disabled one.
        for param in [
            &band.frequency,
            &band.gain,
            &band.q,
            &band.pitch.multiplier,
            &band.dynamics.threshold,
            &band.dynamics.range,
            &band.dynamics.attack,
            &band.dynamics.release,
            &band.dynamics.above_ratio,
            &band.dynamics.below_range,
            &band.dynamics.below_ratio,
            &band.dynamics.detect.frequency,
            &band.dynamics.detect.q,
            &band.dynamics.relative.frequency,
            &band.dynamics.relative.q,
            &band.dynamics.relative_amount,
            &band.dynamics.onsets_amount,
        ] {
            let default = param.default_plain_value();
            if param.value() != default {
                self.set(setter, param, default);
            }
        }
        self.params.clear_placed(index);
    }

    pub(super) fn write_pitch_detection(
        &self,
        setter: &ParamSetter<'_>,
        detection: &PitchDetectionParams,
        control: PitchControl,
    ) {
        self.set(setter, &detection.algorithm, control.algorithm);
        self.set(setter, &detection.profile, control.profile);
        self.set(setter, &detection.target, control.target.mode);
        for (param, value) in [
            (&detection.tracking, control.tracking_ms),
            (&detection.gate, control.gate_db),
            (&detection.clarity, control.clarity * 100.0),
            (&detection.minimum, control.target.minimum),
            (&detection.maximum, control.target.maximum),
            (&detection.reference, control.target.reference),
            (&detection.tolerance, control.target.tolerance),
        ] {
            if param.value() != value {
                self.set(setter, param, value);
            }
        }
    }

    /// Reset all existing bands and active regions back to 0 dB.
    /// Untouched slots are skipped so we don't accidentally mark them as placed.
    pub(crate) fn flatten_all(&self) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        for index in 0..crate::band::BAND_COUNT {
            if !self.band_exists(index) {
                continue;
            }
            self.params.mark_placed(index);
            self.set(&setter, &self.params.bands[index].gain, 0.0);
        }
        for region in &self.params.regions {
            if region.enabled.value() {
                self.set(&setter, &region.gain, 0.0);
            }
        }
    }

    /// Find the next existing band (wrapping around), skipping untouched slots.
    pub(crate) fn step_to_band(&self, from: usize, forwards: bool) -> Option<usize> {
        let count = crate::band::BAND_COUNT;
        (1..=count)
            .map(|step| {
                if forwards {
                    (from + step) % count
                } else {
                    (from + count - step) % count
                }
            })
            .find(|&index| self.band_exists(index))
    }
}
