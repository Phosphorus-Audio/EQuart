use super::relation::{Axis, Diagram, relation, route_title};
use super::*;
use crate::modulation::relation::Curve;
use crate::region::{
    HarmonicSample, MAX_HARMONIC, RegionControl,
    response::{Response as Control, ResponseParams, Scope, Value, whole},
};
use nice_plug::prelude::Enum;

const GRAPH: f32 = 260.0;
const GRAPH_HEIGHT: f32 = 96.0;
const ROUTE: f32 = 176.0;
const SHAPE: f32 = 176.0;
const READING: f32 = (GRAPH - 8.0) / 3.0;
const MAX_CHOICES: usize = Value::WHOLE_TARGETS.len();
pub(in crate::editor) const HEIGHT: f32 = 188.0;
pub(super) const WIDTH: f32 = HEADER_WIDTH + 20.0 + GRAPH + ROUTE + SHAPE + 2.0 * COLUMN_GAP;

struct ResponseChoices {
    scopes: &'static [Scope],
    sources: [Value; MAX_CHOICES],
    source_count: usize,
    targets: [Value; MAX_CHOICES],
    target_count: usize,
}

impl ResponseChoices {
    fn new(control: Control, base: RegionControl) -> Self {
        let scopes = if base.harmonic.enabled {
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

        let mut choices = Self {
            scopes,
            sources: [Value::Gain; MAX_CHOICES],
            source_count: 0,
            targets: [Value::Gain; MAX_CHOICES],
            target_count: 0,
        };

        for &value in sources {
            if value != control.target && value.available(base) {
                choices.sources[choices.source_count] = value;
                choices.source_count += 1;
            }
        }

        for &value in targets {
            if value != control.source && value.available(base) {
                choices.targets[choices.target_count] = value;
                choices.target_count += 1;
            }
        }

        choices
    }

    fn sources(&self) -> &[Value] {
        &self.sources[..self.source_count]
    }

    fn targets(&self) -> &[Value] {
        &self.targets[..self.target_count]
    }
}

#[derive(Clone, Copy)]
struct ResponseControls<'a> {
    param: &'a ResponseParams,
    base: RegionControl,
    control: Control,
    fundamental: Option<f32>,
    rate: f32,
    choices: &'a ResponseChoices,
}

impl EquartEditor {
    pub(super) fn initialize_response(&self, setter: &ParamSetter<'_>, index: usize, slot: usize) {
        let region = &self.params.regions[index];
        let config = region.control();
        let param = &region.responses[slot];
        let target = default_target(Scope::Whole, config);

        self.set(setter, &param.scope, Scope::Whole);
        self.set(setter, &param.source, default_source(Scope::Whole));
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
        let control = param.control();
        let smoothed = self.telemetry.response_region(index, base);
        let pitch = self.telemetry.harmonic_hz(index);
        let rate = self.telemetry.sample_rate();
        let rate = if rate > 0.0 { rate } else { 48_000.0 };

        let effective = whole(smoothed, pitch, rate);
        let fundamental = base.harmonic.fundamental(pitch);
        let choices = ResponseChoices::new(control, base);
        let controls = ResponseControls {
            param,
            base,
            control,
            fundamental,
            rate,
            choices: &choices,
        };

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
                self.response_controls(ui, &setter, &controls);
            },
        );
    }

    fn response_controls(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        controls: &ResponseControls<'_>,
    ) {
        column(ui, ROUTE, |ui| self.response_route(ui, setter, controls));
        column(ui, SHAPE, |ui| self.response_shape(ui, setter, controls));
    }

    fn response_route(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        controls: &ResponseControls<'_>,
    ) {
        let ResponseControls {
            param,
            base,
            control,
            fundamental,
            rate,
            choices,
        } = *controls;
        let scope_changed = param.scope.value() != control.scope;

        ui.small("SCOPE");
        self.pitch_choice(ui, setter, &param.scope, choices.scopes, |scope| {
            Scope::variants()[scope.to_index()]
        });

        ui.small("SOURCE");

        if scope_changed {
            self.set(setter, &param.source, default_source(param.scope.value()));
        } else {
            self.pitch_choice(ui, setter, &param.source, choices.sources(), Value::label);
        }

        ui.small("TARGET");

        if scope_changed {
            self.set(
                setter,
                &param.target,
                default_target(param.scope.value(), base),
            );
        } else {
            self.pitch_choice(ui, setter, &param.target, choices.targets(), Value::label);
        }

        if scope_changed || param.source.value() != control.source {
            let source = param.source.value();
            let value = reference_input(source, base, fundamental, rate);

            self.set(
                setter,
                &param.reference,
                100.0 * source.reference_position(value),
            );
        }
    }

    fn response_shape(
        &self,
        ui: &mut egui::Ui,
        setter: &ParamSetter<'_>,
        controls: &ResponseControls<'_>,
    ) {
        let ResponseControls {
            param,
            base,
            control,
            fundamental,
            rate,
            ..
        } = *controls;

        ui.small("CURVE");
        self.pitch_choice(ui, setter, &param.curve, &Curve::ALL, |curve| {
            Curve::variants()[curve.to_index()]
        });
        self.dynamic_number(ui, setter, "Strength", &param.strength, 0.01);

        ui.small("REFERENCE");

        let mut reference = control.reference;
        let (min, max) = control.source.limits(rate.max(96_000.0));
        let size = Vec2::new(ui.available_width(), ui.spacing().interact_size.y);

        if ui
            .add_sized(
                size,
                egui::DragValue::new(&mut reference)
                    .range(min..=max)
                    .suffix(control.source.unit())
                    .speed(0.1),
            )
            .changed()
        {
            self.set(
                setter,
                &param.reference,
                100.0 * control.source.reference_position(reference),
            );
        }

        let can_lock = fundamental.is_some()
            || !matches!(
                control.source,
                Value::Fundamental | Value::HarmonicFrequency
            );
        let lock = egui::Button::new("Lock reference")
            .min_size(Vec2::new(
                ui.available_width(),
                ui.spacing().interact_size.y,
            ))
            .wrap_mode(egui::TextWrapMode::Extend);

        if ui
            .add_enabled(can_lock, lock)
            .on_hover_text("Capture the current source value")
            .clicked()
        {
            let value = reference_input(control.source, base, fundamental, rate);

            self.set(
                setter,
                &param.reference,
                100.0 * control.source.reference_position(value),
            );
        }
    }
}

fn default_source(scope: Scope) -> Value {
    match scope {
        Scope::Whole => Value::Gain,
        Scope::Harmonic => Value::HarmonicFrequency,
    }
}

fn default_target(scope: Scope, region: RegionControl) -> Value {
    match scope {
        Scope::Whole if region.resonance.enabled => Value::ResonanceQ,
        Scope::Whole => Value::Softness,
        Scope::Harmonic => Value::HarmonicWidth,
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

#[derive(Clone, Copy)]
struct Preview {
    control: Control,
    base: RegionControl,
    effective: RegionControl,
    fundamental: Option<f32>,
    pitch: Option<f32>,
    rate: f32,
    accent: Color32,
}

struct PreviewData {
    values: [[f32; 2]; MAX_HARMONIC],
    samples: [Option<HarmonicSample>; MAX_HARMONIC],
    count: usize,
    inactive: u32,
    base_value: f32,
}

impl PreviewData {
    fn new(base_value: f32) -> Self {
        Self {
            values: [[0.0; 2]; MAX_HARMONIC],
            samples: [None; MAX_HARMONIC],
            count: 0,
            inactive: 0,
            base_value,
        }
    }
}

impl Preview {
    fn data(self) -> PreviewData {
        let Self {
            control,
            base,
            effective,
            fundamental,
            pitch,
            rate,
            ..
        } = self;
        let mut data = PreviewData::new(control.target.shared(base, fundamental).unwrap_or(0.0));

        if control.scope == Scope::Harmonic {
            for sample in effective.harmonics(rate, pitch).into_iter().flatten() {
                if let Some(input) = sample.source(control.source) {
                    let [base, result] = harmonic_values(sample, control.target);

                    if data.count == 0 {
                        data.base_value = base;
                    }

                    data.values[data.count] = [input, result];
                    data.samples[data.count] = Some(sample);

                    if sample.depth == 0.0 {
                        data.inactive |= 1 << data.count;
                    }

                    data.count += 1;
                }
            }
        } else if let Some(input) = control.source.shared(base, fundamental)
            && let Some(output) = control.target.shared(effective, fundamental)
        {
            data.values[0] = [input, output];
            data.count = 1;
        }

        data
    }

    fn draw(self, ui: &mut egui::Ui) {
        let control = self.control;
        let base = self.base;
        let fundamental = self.fundamental;
        let rate = self.rate;
        let accent = self.accent;
        let requires_pitch =
            control.scope == Scope::Harmonic || control.source == Value::Fundamental;

        route_title(ui, control.source.label(), control.target.label());

        if !control.valid(base) {
            ui.label("Choose an available source and target.")
                .on_hover_text("Harmonic and Resonance targets require an active modifier.");

            return;
        }

        if requires_pitch && fundamental.is_none() {
            ui.label("No fundamental signal");

            return;
        }

        let data = self.data();
        let base_value = data.base_value.max(control.target.limits(rate).0);
        let axes = [control.source, control.target].map(|value| Axis {
            scale: value.scale(),
            bounds: value.limits(rate),
        });

        relation(
            ui,
            Diagram {
                axes,
                base: base_value,
                points: &data.values[..data.count],
                inactive: data.inactive,
                accent,
                height: GRAPH_HEIGHT,
            },
            |input| {
                if control.bypass {
                    base_value
                } else {
                    control.output(input, base_value, rate)
                }
            },
            |input, output, hit| {
                if let Some(sample) = hit.and_then(|index| data.samples[index]) {
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
            ui.small(format!("{} harmonics", data.count));

            if data.count > 0 {
                let (min, max) = data.values[..data.count].iter().fold(
                    (f32::INFINITY, f32::NEG_INFINITY),
                    |(min, max), &[_input, value]| (min.min(value), max.max(value)),
                );

                ui.small(format!(
                    "{} to {}",
                    control.target.display(min),
                    control.target.display(max)
                ));
            }
        } else if data.count != 0 {
            let [input, output] = data.values[0];

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 0.0);
                reading(ui, "INPUT", control.source.display(input));
                reading(ui, "BASE", control.target.display(base_value));
                reading(ui, "OUTPUT", control.target.display(output));
            });
        }
    }
}

fn reading(ui: &mut egui::Ui, label: &str, value: String) {
    column(ui, READING, |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.small(label);
        ui.strong(value);
    });
}

fn harmonic_values(sample: HarmonicSample, target: Value) -> [f32; 2] {
    if target == Value::HarmonicWidth {
        [sample.base_width, sample.width]
    } else {
        [sample.base_depth, sample.depth]
    }
}
