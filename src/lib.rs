mod analyzer;
mod audio_path;
mod band;
mod dsp;
mod dynamics;
mod editor;
mod engine;
mod filter;
mod oversample;
mod params;
mod pitch;
mod precision;
mod realtime;
#[cfg(test)]
mod reference;
mod region;
mod telemetry;
#[cfg(test)]
mod tests;

use engine::{Controls, Engine, HostSample};
use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier, create_egui_editor,
};
use params::EquartParams;
use std::sync::{Arc, atomic::Ordering};
use telemetry::Telemetry;

/// shared product & maker names so UI stays synced with plugin metadata
pub const NAME: &str = "PPA EQuart";
pub const VENDOR: &str = "Phosphorus Audio";

pub const EDITOR_SIZE: [f32; 2] = [960.0, 700.0];

/// min window size that still fits settings card and band panel
pub const MIN_EDITOR_SIZE: [f32; 2] = [600.0, 460.0];

pub struct Equart {
    params: Arc<EquartParams>,
    telemetry: Arc<Telemetry>,
    editor_state: Arc<EguiEditorState>,
    engine: Engine,
    /// Latency last handed to the host, so it is only told when it changes.
    reported_latency: u32,
}

impl Default for Equart {
    fn default() -> Self {
        Self {
            params: Arc::new(EquartParams::default()),
            telemetry: Arc::new(Telemetry::default()),
            editor_state: EguiEditorState::from_size(
                LogicalSize::new(EDITOR_SIZE[0], EDITOR_SIZE[1]),
                1.0,
            ),
            engine: Engine::new(48_000.0),
            reported_latency: 0,
        }
    }
}

impl Equart {
    fn controls(&self) -> Controls {
        Controls {
            bands: std::array::from_fn(|i| self.params.control(i)),
            regions: std::array::from_fn(|i| self.params.regions[i].control()),
            output_db: self.params.output.value(),
            mix: self.params.mix.value() * 0.01,
            bypass: self.params.bypass.value(),
            oversampling: self.params.oversampling.value(),
            precision: self.params.precision.value(),
        }
    }
}

impl Plugin for Equart {
    const NAME: &'static str = crate::NAME;
    const VENDOR: &'static str = crate::VENDOR;
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[NonZeroU32::new(2).expect("two channels is nonzero")],
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[NonZeroU32::MIN],
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    // params are read once per block and smoothed, no need for sample-accurate automation
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type Editor = EguiEditor<editor::EquartEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn filter_state(state: &mut PluginState) {
        use nice_plug::plugin::ParamValue;

        state
            .params
            .entry("dsp_precision".into())
            .or_insert_with(|| ParamValue::String("f32".into()));

        // Depth was stored as a positive attenuation. Gain uses signed dB.
        for index in 1..=region::REGION_COUNT {
            if let Some(ParamValue::F32(depth)) =
                state.params.remove(&format!("region_depth_{index}"))
            {
                state
                    .params
                    .entry(format!("region_gain_{index}"))
                    .or_insert(ParamValue::F32(-depth));
            }
        }
        region::HarmonicControl::migrate_state(state);

        let multiplier = pitch::PitchControl::default().multiplier;
        for index in 1..=band::BAND_COUNT {
            state
                .params
                .entry(format!("pitch_multiplier_{index}"))
                .or_insert(ParamValue::F32(multiplier));
        }
    }

    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Self::Editor> {
        let percent = self.params.ui_scale_percent.load(Ordering::Relaxed);
        let scale = if params::UI_SCALES.contains(&percent) {
            percent as f32 / 100.0
        } else {
            1.0
        };
        self.editor_state
            .set_size(LogicalSize::new(EDITOR_SIZE[0], EDITOR_SIZE[1]));
        self.editor_state.set_zoom_factor(scale);

        // editor drives its own frame rate, no extra notifier needed
        create_egui_editor(
            self.editor_state.clone(),
            RepaintNotifier::new(),
            editor_settings(),
            editor::EquartEditor::new(self.params.clone(), self.telemetry.clone()),
        )
    }

    fn activate(
        &mut self,
        _: &AudioIOLayout,
        config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        if !config.sample_rate.is_finite() || config.sample_rate < 8_000.0 {
            return false;
        }

        self.engine = Engine::new(config.sample_rate);
        self.engine
            .set_oversampling(self.params.oversampling.value());
        self.reported_latency = self.engine.latency();
        context.set_latency_samples(self.reported_latency);
        self.telemetry.publish(&self.engine, true);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
        self.telemetry.publish(&self.engine, true);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        auxiliary: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.process_audio(buffer, auxiliary, context)
    }
}

impl Equart {
    fn process_audio<T: HostSample>(
        &mut self,
        buffer: &mut Buffer<'_, T>,
        auxiliary: &mut AuxiliaryBuffers<'_, T>,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let _denormal_guard = realtime::DenormalGuard::new();
        let spectrum_revision = self.engine.analyzer.revision;
        self.engine.retune(context.transport().sample_rate);
        let open = self.editor_state.is_open();
        self.engine.delta_request = if open {
            self.telemetry.delta_region()
        } else {
            None
        };
        self.engine.analyze = open && self.params.analyzer.load(Ordering::Relaxed);
        let settings = analyzer::AnalyzerSettings::read(&self.params);
        self.engine.analyzer.apply_settings(settings);
        self.engine.output_analyzer.apply_settings(settings);

        // read params once per block instead of per-sample
        let controls = self.controls();
        let channels = buffer.as_slice();
        let sidechain = auxiliary
            .inputs
            .first()
            .and_then(|bus| match bus.as_slice_immutable() {
                [mono] => Some([&mono[..], &mono[..]]),
                [left, right, ..] => Some([&left[..], &right[..]]),
                [] => None,
            });

        match channels.len() {
            0 => {}
            1 => self.engine.process_mono(channels[0], sidechain, &controls),
            _ => {
                let (left, right) = channels.split_at_mut(1);
                self.engine
                    .process_block_with_sidechain(left[0], right[0], sidechain, &controls);
            }
        }

        // tell host when oversampling latency changes
        let latency = self.engine.latency();
        if latency != self.reported_latency {
            self.reported_latency = latency;
            context.set_latency_samples(latency);
        }

        if let Some(fault) = self.engine.fault.take() {
            return ProcessStatus::Error(fault);
        }

        if open {
            self.telemetry.publish(
                &self.engine,
                spectrum_revision != self.engine.analyzer.revision,
            );
        }

        ProcessStatus::Normal
    }
}

fn editor_settings() -> EguiNiceSettings {
    // layout stretches with window rect down to the floor the cards and the panel need.
    let resize_hint = nice_plug::editor::ResizeHint::RESIZABLE
        .with_min_logical_size(LogicalSize::new(MIN_EDITOR_SIZE[0], MIN_EDITOR_SIZE[1]));

    #[cfg(target_os = "windows")]
    {
        let mut graphics = nice_plug_egui::GraphicsConfig::default();
        // Fifo, so no frame ever tears: the curve, the handles and the
        // spectrum are all hand-drawn lines, and a torn one is obvious. It is
        // not what paces the editor -- baseview's frame pacer does that, from
        // the compositor's clock -- but a swapchain allowed to run ahead of
        // the display would still tear, and one allowed to lag would show a
        // picture older than the pointer.
        graphics.wgpu_options.surface.present_mode = egui_wgpu::wgpu::PresentMode::Fifo;
        // At most two frames in flight: enough to keep the GPU fed, few enough
        // that the picture on screen is never more than a frame behind the
        // pointer.
        graphics.wgpu_options.surface.desired_maximum_frame_latency = Some(2);
        let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = egui_wgpu::wgpu::Backends::DX12;
        graphics.wgpu_options.wgpu_setup = setup.into();
        EguiNiceSettings::new()
            .with_graphics_config(graphics)
            .with_resize_hint(resize_hint)
    }

    #[cfg(not(target_os = "windows"))]
    EguiNiceSettings::new().with_resize_hint(resize_hint)
}

impl ClapPlugin for Equart {
    const CLAP_ID: &'static str = "audio.equart.eq";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Good EQ by PPA");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Equalizer,
        ClapFeature::Stereo,
        ClapFeature::Mono,
    ];
}

impl Vst3Plugin for Equart {
    const VST3_CLASS_ID: [u8; 16] = *b"PPA_EQUART__PLUG";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Eq];
    const SUPPORTS_SAMPLE_64: bool = true;

    fn process_f64(
        &mut self,
        buffer: &mut Buffer<'_, f64>,
        auxiliary: &mut AuxiliaryBuffers<'_, f64>,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.process_audio(buffer, auxiliary, context)
    }
}

nice_export_clap!(Equart);
nice_export_vst3!(Equart);
