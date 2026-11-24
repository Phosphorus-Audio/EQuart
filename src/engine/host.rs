use super::{Controls, Engine, OutputMix};
use crate::band::COEFF_FRAMES;
use crate::precision::{Precision, Sample};

pub(crate) trait HostSample: Sample {
    const NATIVE_DOUBLE: bool;

    fn analysis<'a>(samples: &'a [Self], scratch: &'a mut [f32]) -> &'a [f32];
    fn render(
        engine: &mut Engine,
        left: &mut [Self],
        right: &mut [Self],
        controls: &Controls,
        mix: OutputMix,
    );
}

impl HostSample for f32 {
    const NATIVE_DOUBLE: bool = false;

    fn analysis<'a>(samples: &'a [Self], _: &'a mut [f32]) -> &'a [f32] {
        samples
    }

    fn render(
        engine: &mut Engine,
        left: &mut [Self],
        right: &mut [Self],
        controls: &Controls,
        mix: OutputMix,
    ) {
        if engine.precision == Precision::Single {
            engine.single.process(
                left,
                right,
                &controls.bands,
                mix,
                &mut engine.delta_mix,
                &mut engine.region_dynamics,
            );
            return;
        }

        let mut wide_left = [0.0; COEFF_FRAMES];
        let mut wide_right = [0.0; COEFF_FRAMES];
        let wide_left = &mut wide_left[..left.len()];
        let wide_right = &mut wide_right[..right.len()];

        for (wide, sample) in wide_left
            .iter_mut()
            .chain(wide_right.iter_mut())
            .zip(left.iter().chain(right.iter()))
        {
            *wide = f64::from(*sample);
        }

        f64::render(engine, wide_left, wide_right, controls, mix);

        for (sample, wide) in left
            .iter_mut()
            .chain(right.iter_mut())
            .zip(wide_left.iter().chain(wide_right.iter()))
        {
            *sample = *wide as f32;
        }
    }
}

impl HostSample for f64 {
    const NATIVE_DOUBLE: bool = true;

    fn analysis<'a>(samples: &'a [Self], scratch: &'a mut [f32]) -> &'a [f32] {
        let scratch = &mut scratch[..samples.len()];
        for (out, sample) in scratch.iter_mut().zip(samples) {
            *out = *sample as f32;
        }
        scratch
    }

    fn render(
        engine: &mut Engine,
        left: &mut [Self],
        right: &mut [Self],
        controls: &Controls,
        mix: OutputMix,
    ) {
        engine.double.process(
            left,
            right,
            &controls.bands,
            mix,
            &mut engine.delta_mix,
            &mut engine.region_dynamics,
        );
    }
}
