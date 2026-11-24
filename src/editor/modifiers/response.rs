use super::relation::{Axis, Diagram, relation};
use super::*;
use crate::modulation::relation::Curve;
use crate::region::{
    HarmonicSample, MAX_HARMONIC, RegionControl,
    response::{Response as Control, Scope, Value, whole},
};
use nice_plug::prelude::Enum;

const GRAPH: f32 = 192.0;
const CONTROL: f32 = 166.0;
const SETTINGS: f32 = 146.0;
pub(super) const WIDTH: f32 = HEADER_WIDTH + 20.0 + GRAPH + CONTROL + SETTINGS + 2.0 * COLUMN_GAP;

impl EquartEditor {
    pub(super) fn initialize_response(&self, setter: &ParamSetter<'_>, index: usize, slot: usize) {
        let region = &self.params.regions[index];
        let config = region.control();
        let param = &region.responses[slot];
        let target = if config.resonance.enabled {
            Value::ResonanceQ
        } else {
            Value::Softness
        };
        self.set(setter, &param.scope, Scope::Whole);
        self.set(setter, &param.source, Value::Gain);
        self.set(setter, &param.target, target);
        self.set(setter, &param.curve, Curve::Power);
        self.set(setter, &param.strength, 0.0);
        self.set(
            setter,
            &param.reference,
            Value::Gain.reference_position(config.gain) * 100.0,
        );
    }

    pub(super) fn response_modifier(
        &self,
        ui: &mut egui::Ui,
        rect: Rect,
        index: usize,
        slot: usize,
    ) {
        let Some(gui) = &self.gui else { return };
        let setter = gui.param_setter();
        let params = &self.params.regions[index];
        let param = &params.responses[slot];
        let accent = self.theme.region[index];
        let modifier = Modifier::Response(slot);
        let header = Rect::from_min_size(rect.min, Vec2::new(HEADER_WIDTH, rect.height()));
        let active = !param.bypass.value();
        let (toggle, remove) = modifier_header(ui, header, index, modifier.label(), active, accent);

        if toggle {
            self.set(&setter, &param.bypass, active);
        }

        if remove {
            self.set(&setter, &param.enabled, false);
            return;
        }

        if params
            .responses
            .iter()
            .all(|response| response.enabled.value())
        {
            let handle = ui
                .interact(
                    Rect::from_center_size(header.center(), Vec2::new(HEADER_WIDTH, 56.0)),
                    ui.make_persistent_id(("response-order", index, slot)),
                    Sense::click_and_drag(),
                )
                .on_hover_cursor(egui::CursorIcon::Grab)
                .on_hover_text("Drag onto the other Response or double-click to swap their order.");
            handle.dnd_set_drag_payload((index, slot));

            let hovering_other = handle
                .dnd_hover_payload::<(usize, usize)>()
                .is_some_and(|source| *source == (index, 1 - slot));
            if hovering_other {
                ui.painter().rect_stroke(
                    handle.rect,
                    4.0,
                    Stroke::new(1.0, accent),
                    egui::StrokeKind::Inside,
                );
            }

            if handle.double_clicked()
                || (hovering_other && handle.dnd_release_payload::<(usize, usize)>().is_some())
            {
                self.set(
                    &setter,
                    &params.reverse_responses,
                    !params.reverse_responses.value(),
                );
            }
        }

        let mut content = rect.shrink2(Vec2::new(10.0, 7.0));
        content.min.x += HEADER_WIDTH;
        let base = params.control();
        let smoothed = self.telemetry.response_region(index, base);
        let control = param.control();
        let pitch = self.telemetry.harmonic_hz(index);
        let rate = self.telemetry.sample_rate();
        let rate = if rate > 0.0 { rate } else { 48_000.0 };
        let effective = whole(smoothed, pitch, rate);
        let fundamental = base.harmonic.fundamental(pitch);
        let choices = if base.harmonic.enabled {
            &[Scope::Whole, Scope::Harmonic][..]
        } else {
            &[Scope::Whole][..]
        };
        let (sources, targets) = match control.scope {
            Scope::Whole => (
                Value::WHOLE_SOURCES.as_slice(),
                Value::WHOLE_TARGETS.as_slice(),
            ),
            Scope::Harmonic => (
                Value::HARMONIC_SOURCES.as_slice(),
                Value::HARMONIC_TARGETS.as_slice(),
            ),
        };
        let sources = sources
            .iter()
            .copied()
            .filter(|value| *value != control.target && value.available(base))
            .collect::<Vec<_>>();
        let targets = targets
            .iter()
            .copied()
            .filter(|value| *value != control.source && value.available(base))
            .collect::<Vec<_>>();

        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("response", index, slot))
                .max_rect(content)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(COLUMN_GAP, 4.0);
                column(ui, GRAPH, |ui| {
                    let view = Preview {
                        control,
                        base,
                        effective,
                        fundamental,
                        pitch,
                        rate,
                        accent,
                    };
                    view.draw(ui);
                });
                column(ui, CONTROL, |ui| {
                    self.pitch_choice(ui, &setter, &param.scope, choices, |scope| {
                        Scope::variants()[scope.to_index()]
                    });
                    if param.scope.value() != control.scope {
                        let (source, target) = if param.scope.value() == Scope::Harmonic {
                            (Value::HarmonicFrequency, Value::HarmonicWidth)
                        } else if base.resonance.enabled {
                            (Value::Gain, Value::ResonanceQ)
                        } else {
                            (Value::Gain, Value::Softness)
                        };
                        self.set(&setter, &param.source, source);
                        self.set(&setter, &param.target, target);
                    } else {
                        self.pitch_choice(ui, &setter, &param.source, &sources, Value::label);
                        self.pitch_choice(ui, &setter, &param.target, &targets, Value::label);
                    }
                    if param.scope.value() != control.scope
                        || param.source.value() != control.source
                    {
                        let source = param.source.value();
                        let value = reference_input(source, base, fundamental, rate);
                        self.set(
                            &setter,
                            &param.reference,
                            100.0 * source.reference_position(value),
                        );
                    }
                    self.dynamic_number(ui, &setter, "Strength", &param.strength, 0.01);
                });
                column(ui, SETTINGS, |ui| {
                    self.pitch_choice(ui, &setter, &param.curve, &Curve::ALL, |curve| {
                        Curve::variants()[curve.to_index()]
                    });
                    let mut reference = control.reference;
                    let (min, max) = control.source.limits(rate.max(96_000.0));
                    ui.horizontal(|ui| {
                        ui.label("Ref");
                        if ui
                            .add(
                                egui::DragValue::new(&mut reference)
                                    .range(min..=max)
                                    .suffix(control.source.unit())
                                    .speed(0.1),
                            )
                            .changed()
                        {
                            self.set(
                                &setter,
                                &param.reference,
                                100.0 * control.source.reference_position(reference),
                            );
                        }
                    });
                    let can_lock = fundamental.is_some()
                        || !matches!(
                            control.source,
                            Value::Fundamental | Value::HarmonicFrequency
                        );
                    if ui
                        .add_enabled(can_lock, egui::Button::new("Lock reference"))
                        .clicked()
                    {
                        let value = reference_input(control.source, base, fundamental, rate);
                        self.set(
                            &setter,
                            &param.reference,
                            100.0 * control.source.reference_position(value),
                        );
                    }
                });
            },
        );
    }
}

fn reference_input(
    source: Value,
    region: RegionControl,
    fundamental: Option<f32>,
    rate: f32,
) -> f32 {
    let first = region.harmonic.first.min(region.harmonic.last).max(1) as f32;
    match source {
        Value::HarmonicIndex => first,
        Value::HarmonicFrequency => fundamental.map_or(region.bounds()[0], |hz| hz * first),
        Value::HarmonicDepth => region.gain.abs(),
        _ => source
            .shared(region, fundamental)
            .unwrap_or_else(|| source.limits(rate).0),
    }
}

struct Preview {
    control: Control,
    base: RegionControl,
    effective: RegionControl,
    fundamental: Option<f32>,
    pitch: Option<f32>,
    rate: f32,
    accent: Color32,
}

impl Preview {
    fn draw(self, ui: &mut egui::Ui) {
        let Self {
            control,
            base,
            effective,
            fundamental,
            pitch,
            rate,
            accent,
        } = self;
        let requires_pitch =
            control.scope == Scope::Harmonic || control.source == Value::Fundamental;
        let mut values = [[0.0; 2]; MAX_HARMONIC];
        let mut samples = [None; MAX_HARMONIC];
        let mut count = 0;
        let mut inactive = 0;
        let mut base_value = control.target.shared(base, fundamental).unwrap_or(0.0);

        ui.strong(control.target.label());
        if !control.valid(base) {
            ui.label("Choose an available source and target.")
                .on_hover_text("Harmonic and Resonance targets require an active modifier.");
        } else if requires_pitch && fundamental.is_none() {
            ui.label("No fundamental signal");
        } else if control.scope == Scope::Harmonic {
            for sample in effective.harmonics(rate, pitch).into_iter().flatten() {
                if let Some(input) = sample.source(control.source) {
                    let [base, result] = harmonic_values(sample, control.target);
                    if count == 0 {
                        base_value = base;
                    }
                    values[count] = [input, result];
                    samples[count] = Some(sample);
                    if sample.depth == 0.0 {
                        inactive |= 1 << count;
                    }
                    count += 1;
                }
            }
        } else if let Some(input) = control.source.shared(base, fundamental)
            && let Some(output) = control.target.shared(effective, fundamental)
        {
            values[0] = [input, output];
            count = 1;
        }

        if !control.valid(base) || (requires_pitch && fundamental.is_none()) {
            return;
        }

        let axes = [control.source, control.target].map(|value| Axis {
            scale: value.scale(),
            bounds: value.limits(rate),
        });
        relation(
            ui,
            Diagram {
                axes,
                base: base_value.max(control.target.limits(rate).0),
                points: &values[..count],
                inactive,
                accent,
            },
            |input| {
                if control.bypass {
                    base_value
                } else {
                    control.output(input, base_value.max(control.target.limits(rate).0), rate)
                }
            },
            |input, output, hit| {
                if let Some(sample) = hit.and_then(|index| samples[index]) {
                    let [base, effective] = harmonic_values(sample, control.target);
                    return format!(
                        "#{} · {:.1} Hz\nBase {} · Effective {}",
                        sample.number,
                        sample.frequency,
                        control.target.display(base),
                        control.target.display(effective)
                    );
                }

                format!(
                    "{} to {}",
                    control.source.display(input),
                    control.target.display(output)
                )
            },
        );

        if control.scope == Scope::Harmonic {
            ui.small(format!("{count} harmonics"));
            if count > 0 {
                let (min, max) = values[..count].iter().fold(
                    (f32::INFINITY, f32::NEG_INFINITY),
                    |(min, max), &[_input, value]| (min.min(value), max.max(value)),
                );
                ui.small(format!(
                    "{} to {}",
                    control.target.display(min),
                    control.target.display(max)
                ));
            }
        } else if count != 0 {
            ui.small(format!("Input {}", control.source.display(values[0][0])));
            ui.small(format!(
                "Base {} · Out {}",
                control.target.display(base_value),
                control.target.display(values[0][1])
            ));
        }
    }
}

fn harmonic_values(sample: HarmonicSample, target: Value) -> [f32; 2] {
    if target == Value::HarmonicWidth {
        [sample.base_width, sample.width]
    } else {
        [sample.base_depth, sample.depth]
    }
}
