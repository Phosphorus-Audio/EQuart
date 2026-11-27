use super::relation::route_title;
use super::*;
use crate::modulation::{Division, Lfo, Mapping, Target, Timing, Wave};
use crate::processing_mode::ProcessingMode;
use nice_plug::prelude::Enum;

const GRAPH_WIDTH: f32 = 176.0;
const CONTROL_WIDTH: f32 = 152.0;
pub(super) const WIDTH: f32 =
    HEADER_WIDTH + 20.0 + GRAPH_WIDTH + 2.0 * (CONTROL_WIDTH + COLUMN_GAP);

impl EquartEditor {
    fn modulation_choice<T: Enum + Copy + PartialEq + 'static>(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        param: &EnumParam<T>,
        choices: &[T],
        label: impl Fn(T) -> &'static str,
    ) {
        let combo = egui::ComboBox::from_id_salt(param.name())
            .width(ui.available_width())
            .wrap_mode(egui::TextWrapMode::Extend);
        self.modifier_choice(ui, combo, setter, param, choices, label);
    }

    pub(super) fn lock_mapping_references(&self, setter: &ParamSetter<'_>, band: &BandParams) {
        for (source, base) in [
            (Target::Frequency, band.frequency.value()),
            (Target::Gain, band.gain.value()),
            (Target::Q, band.q.value()),
        ] {
            self.set(setter, band.mapping.reference(source), base);
        }
    }

    fn modulation_header(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        index: usize,
        modifier: Modifier,
        setter: &ParamSetter<'_>,
        switches: [&BoolParam; 2],
    ) -> Option<Rect> {
        let [enabled, bypass] = switches;
        let active = !bypass.value();
        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let (toggle, remove) = modifier_header(
            ui,
            header,
            index,
            modifier.label(),
            active,
            self.theme.band[index],
        );

        if toggle {
            self.set(setter, bypass, active);
        }

        if remove {
            self.set(setter, enabled, false);
            return None;
        }

        let mut content = rect.shrink2(Vec2::new(10.0, 7.0));
        content.min.x += HEADER_WIDTH;
        Some(content)
    }

    pub(super) fn mapping_modifier(&self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let params = self.params.band(index);
        let mapping = &params.mapping;
        let Some(content) = self.modulation_header(
            ui,
            rect,
            index,
            Modifier::Mapping,
            &setter,
            [&mapping.enabled, &mapping.bypass],
        ) else {
            return;
        };

        let band = params.control();
        let control = mapping.control();
        let live = self.telemetry.modulation(index);
        let host_rate = self.telemetry.sample_rate();
        let rate = if host_rate > 0.0 { host_rate } else { 48_000.0 };
        let source = live.map_or_else(|| control.source.value(band), |data| data.mapping_input);
        let base = control.target.value(band);
        let stage_base = live.map_or(base, |data| data.mapping_base);
        let effective = live.map_or_else(
            || control.target.value(band),
            |data| {
                let mut effective = band;
                data.apply(&mut effective);
                control.target.value(effective)
            },
        );
        let accent = self.theme.band[index];

        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("mapping", index))
                .max_rect(content)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                column(ui, GRAPH_WIDTH, |ui| {
                    route_title(ui, control.source.label(), control.target.label());
                    let graph = if control.bypass
                        || !control.target.supported(band)
                        || !control.source.supported(band)
                    {
                        Mapping {
                            strength: 0.0,
                            ..control
                        }
                    } else {
                        control
                    };
                    mapping_graph(ui, graph, [source, stage_base, effective], rate, accent);
                    ui.small(format!("Input  {}", control.source.display(source)));
                    ui.small(format!(
                        "Base {}  ·  Out {}",
                        control.target.display(base),
                        control.target.display(effective)
                    ));
                });
                column(ui, CONTROL_WIDTH, |ui| {
                    let choices = Target::ALL
                        .into_iter()
                        .filter(|target| *target != control.target && target.supported(band))
                        .collect::<Vec<_>>();
                    ui.small("Source");
                    self.modulation_choice(ui, &setter, &mapping.source, &choices, Target::label);

                    let choices = Target::ALL
                        .into_iter()
                        .filter(|target| *target != control.source && target.supported(band))
                        .collect::<Vec<_>>();
                    ui.small("Target");
                    self.modulation_choice(ui, &setter, &mapping.target, &choices, Target::label);
                    let response =
                        self.dynamic_number(ui, &setter, "Strength", &mapping.strength, 0.01);
                    if response.double_clicked() {
                        self.set(&setter, &mapping.strength, 0.0);
                    }
                });
                column(ui, CONTROL_WIDTH, |ui| {
                    ui.small("Power curve");
                    let preset = match (control.source, control.target, control.strength) {
                        (Target::Frequency, Target::Q, 1.0) => "Constant BW",
                        (Target::Frequency, Target::Q, -1.0) => "Inverse Q",
                        _ => "Custom",
                    };
                    let preset_button = control_button(ui, preset);
                    egui::containers::menu::MenuButton::from_button(preset_button).ui(ui, |ui| {
                        for (label, strength) in [("Constant Bandwidth", 1.0), ("Inverse Q", -1.0)]
                        {
                            if ui
                                .add_enabled(Target::Q.supported(band), egui::Button::new(label))
                                .clicked()
                            {
                                self.set(&setter, &mapping.source, Target::Frequency);
                                self.set(&setter, &mapping.target, Target::Q);
                                self.set(&setter, &mapping.strength, strength);
                                ui.close();
                            }
                        }
                        if ui.selectable_label(preset == "Custom", "Custom").clicked() {
                            if preset != "Custom" {
                                self.set(&setter, &mapping.strength, 0.0);
                            }
                            ui.close();
                        }
                    });
                    ui.small("Reference");
                    let lock_button = control_button(ui, "Lock reference");
                    if ui
                        .add(lock_button)
                        .on_hover_text("Capture the current base value")
                        .clicked()
                    {
                        self.set(
                            &setter,
                            mapping.reference(control.source),
                            control.source.value(band),
                        );
                    }
                    self.dynamic_number(ui, &setter, "Ref", mapping.reference(control.source), 0.1);
                });
            },
        );
    }

    pub(super) fn lfo_modifier(&self, ui: &mut egui::Ui, rect: Rect, index: usize) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let band = self.params.band(index);
        let params = &band.lfo;
        let Some(content) = self.modulation_header(
            ui,
            rect,
            index,
            Modifier::Lfo,
            &setter,
            [&params.enabled, &params.bypass],
        ) else {
            return;
        };

        let control = params.control();
        let telemetry = self.telemetry.modulation(index).unwrap_or_default();
        let accent = self.theme.band[index];
        let linear = self.params.global.processing_mode.value() == ProcessingMode::LinearPhase;
        let targets = Target::ALL
            .into_iter()
            .filter(|target| target.supported(band.control()))
            .collect::<Vec<_>>();

        ui.scope_builder(
            egui::UiBuilder::new().id_salt(("lfo", index)).max_rect(content)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                column(ui, GRAPH_WIDTH, |ui| {
                    ui.strong(format!("LFO · {}", control.target.label()));
                    lfo_graph(ui, control, telemetry.phase, telemetry.random, telemetry.lfo_offset, accent);
                    ui.small(format!("Current  {}", control.target.offset_text(telemetry.lfo_offset)));
                    if linear {
                        ui.small("Linear Phase · max 1 Hz").on_hover_text(
                            "The waveform is smoothed and the FIR responds at its kernel update rate.",
                        );
                    }
                });
                column(ui, CONTROL_WIDTH, |ui| {
                    self.modulation_choice(ui, &setter, &params.target, &targets, Target::label);
                    self.modulation_choice(ui, &setter, &params.wave, &Wave::ALL, |wave| Wave::variants()[wave.to_index()]);
                    let speed = if control.target == Target::Q { 0.02 } else { 0.1 };
                    self.dynamic_number(ui, &setter, "Depth", params.depth(control.target), speed);
                    if !targets.contains(&control.target) {
                        ui.small("Target unavailable for this filter");
                    }
                });
                column(ui, CONTROL_WIDTH, |ui| {
                    let mode = if control.sync { "Sync" } else { "Free" };
                    let mode_button = control_button(ui, mode);
                    if ui.add(mode_button).on_hover_text("Switch between tempo sync and Hertz").clicked() {
                        self.set(&setter, &params.sync, !control.sync);
                    }
                    if control.sync {
                        self.modulation_choice(ui, &setter, &params.division, &Division::ALL, |d| Division::variants()[d.to_index()]);
                    } else {
                        self.dynamic_number(ui, &setter, "Rate", &params.rate, 0.02);
                    }
                    let timing_button = control_button(ui, "Phase & timing");
                    egui::containers::menu::MenuButton::from_button(timing_button).ui(ui, |ui| {
                        self.dynamic_number(ui, &setter, "Phase", &params.phase, 1.0);
                        for (param, label) in [
                            (&params.unipolar, "Unipolar"),
                            (&params.transport, "Transport phase"),
                        ] {
                            let mut enabled = param.value();
                            if ui.checkbox(&mut enabled, label).changed() {
                                self.set(&setter, param, enabled);
                            }
                        }
                        if control.sync {
                            self.pitch_choice(ui, &setter, &params.timing, &Timing::ALL, |t| Timing::variants()[t.to_index()]);
                        }
                    });
                });
            },
        );
    }
}

fn control_button<'a>(ui: &egui::Ui, label: &'a str) -> egui::Button<'a> {
    egui::Button::new(label)
        .min_size(Vec2::new(
            ui.available_width(),
            ui.spacing().interact_size.y,
        ))
        .wrap_mode(egui::TextWrapMode::Extend)
}

fn mapping_graph(
    ui: &mut egui::Ui,
    mapping: Mapping,
    [source, base, effective]: [f32; 3],
    rate: f32,
    accent: Color32,
) {
    use super::relation::{Axis, DEFAULT_HEIGHT, Diagram, relation};
    let axes = [mapping.source, mapping.target].map(|target| Axis {
        scale: target.scale(),
        bounds: target.limits(rate),
    });
    relation(
        ui,
        Diagram {
            axes,
            base,
            points: &[[source, effective]],
            inactive: 0,
            accent,
            height: DEFAULT_HEIGHT,
        },
        |input| mapping.output(input, base, rate),
        |input, output, _| {
            format!(
                "{} to {}",
                mapping.source.display(input),
                mapping.target.display(output)
            )
        },
    );
}

fn lfo_graph(ui: &mut egui::Ui, lfo: Lfo, phase: f32, random: f32, offset: f32, accent: Color32) {
    let (rect, _, palette) = super::relation::graph(ui);
    let y = |sample| {
        let t = if lfo.unipolar {
            lfo.polar(sample)
        } else {
            (sample + 1.0) * 0.5
        };
        egui::lerp(rect.bottom()..=rect.top(), t)
    };
    let p = ui.painter_at(rect.expand(2.0));
    p.hline(
        rect.x_range(),
        if lfo.unipolar { rect.bottom() } else { y(0.0) },
        Stroke::new(1.0, palette.grid.zero),
    );
    let points = (0..=64)
        .map(|step| {
            let t = step as f32 / 64.0;
            Pos2::new(egui::lerp(rect.x_range(), t), y(lfo.wave.sample(t, random)))
        })
        .collect();
    let tint = accent.gamma_multiply(if lfo.bypass {
        0.45
    } else {
        0.65 + 0.35 * (lfo.depth / 3.0).clamp(0.0, 1.0)
    });
    p.add(egui::Shape::line(points, Stroke::new(1.6, tint)));
    let x = egui::lerp(rect.x_range(), phase);
    p.vline(
        x,
        rect.y_range(),
        Stroke::new(1.0, tint.gamma_multiply(0.4)),
    );
    let sample = if lfo.depth > 0.0 {
        let amount = offset / lfo.depth;
        if lfo.unipolar {
            amount * 2.0 - 1.0
        } else {
            amount
        }
    } else {
        lfo.wave.sample(phase, random)
    };
    p.circle_filled(Pos2::new(x, y(sample.clamp(-1.0, 1.0))), 3.0, tint);
}
