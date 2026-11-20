use crate::dsp::{self, Kernel, State};
use crate::filter::Coefficients;
use nice_plug::prelude::Enum;
use realfft::num_traits::{Float, FloatConst, NumAssign};

#[derive(Clone, Copy, Debug, Default, Enum, Eq, PartialEq)]
pub(crate) enum Precision {
    #[default]
    #[id = "f32"]
    #[name = "32-bit"]
    Single,
    #[id = "f64"]
    #[name = "64-bit"]
    Double,
}

/// Numeric operations shared by the two audio paths.
pub(crate) trait Sample: Float + FloatConst + NumAssign {
    type BlockKernel;

    fn value(value: f64) -> Self;
    fn double(self) -> f64;
    fn kernel(coefficients: &Coefficients<Self>) -> Self::BlockKernel;
    fn run_block(
        kernel: &Self::BlockKernel,
        coefficients: &Coefficients<Self>,
        state: &mut State<Self>,
        audio: &mut [Self],
    );

    fn scale(audio: &mut [Self], gain: Self) {
        for sample in audio {
            *sample *= gain;
        }
    }
}

impl Sample for f32 {
    type BlockKernel = Kernel;

    fn value(value: f64) -> Self {
        value as Self
    }

    fn double(self) -> f64 {
        f64::from(self)
    }

    fn kernel(coefficients: &Coefficients<Self>) -> Self::BlockKernel {
        Kernel::new(coefficients)
    }

    fn run_block(
        kernel: &Self::BlockKernel,
        _: &Coefficients<Self>,
        state: &mut State<Self>,
        audio: &mut [Self],
    ) {
        kernel.apply_slice(state, audio);
    }

    fn scale(audio: &mut [Self], gain: Self) {
        dsp::scale(audio, gain);
    }
}

impl Sample for f64 {
    type BlockKernel = ();

    fn value(value: f64) -> Self {
        value
    }

    fn double(self) -> f64 {
        self
    }

    fn kernel(_: &Coefficients<Self>) -> Self::BlockKernel {}

    fn run_block(
        _: &Self::BlockKernel,
        coefficients: &Coefficients<Self>,
        state: &mut State<Self>,
        audio: &mut [Self],
    ) {
        for sample in audio {
            *sample = dsp::step(state, coefficients, *sample);
        }
    }
}

pub(crate) fn convert<T: Sample, S: Sample, const N: usize>(samples: [S; N]) -> [T; N] {
    samples.map(|sample| T::value(sample.double()))
}
