use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex, num_complex::Complex64};

pub(super) const HOP: usize = 256;
const FFT: usize = 2 * HOP;
const BINS: usize = HOP + 1;
const PARTITIONS: usize = 2 * crate::processing_mode::LinearQuality::MAX_CENTER / HOP + 1;

type Spectrum = [Complex64; BINS];

pub(super) struct Kernel {
    partitions: [Vec<Spectrum>; 8],
    pub(super) routes: [Option<usize>; 8],
    pub(super) half_taps: [Vec<f64>; 4],
    pub(super) identity: bool,
    pub(super) last_tap: bool,
    pub(super) channel_modes: u8,
}

impl Kernel {
    pub(super) fn new() -> Self {
        Self {
            partitions: std::array::from_fn(|_| vec![[Complex64::default(); BINS]; PARTITIONS]),
            routes: [None; 8],
            identity: false,
            last_tap: false,
            channel_modes: 0,
            half_taps: std::array::from_fn(|_| {
                vec![0.0; crate::processing_mode::LinearQuality::MAX_CENTER + 1]
            }),
        }
    }
}

pub(super) struct Plans {
    forward: Arc<dyn RealToComplex<f64>>,
    inverse: Arc<dyn ComplexToReal<f64>>,
}

impl Plans {
    pub(super) fn new(planner: &mut RealFftPlanner<f64>) -> Self {
        Self {
            forward: planner.plan_fft_forward(FFT),
            inverse: planner.plan_fft_inverse(FFT),
        }
    }
}

pub(super) struct Convolution {
    plans: &'static Plans,
    history: [Vec<Spectrum>; 2],
    head: usize,
    input: [[f64; FFT]; 2],
    bins: Spectrum,
    work: [f64; FFT],
    scratch: Vec<Complex64>,
    fade: [f64; HOP],
}

impl Convolution {
    pub(super) fn new(plans: &'static Plans) -> Self {
        let scratch = plans
            .forward
            .get_scratch_len()
            .max(plans.inverse.get_scratch_len());

        Self {
            plans,
            history: std::array::from_fn(|_| vec![[Complex64::default(); BINS]; PARTITIONS]),
            head: 0,
            input: [[0.0; FFT]; 2],
            bins: [Complex64::default(); BINS],
            work: [0.0; FFT],
            scratch: vec![Complex64::default(); scratch],
            fade: std::array::from_fn(|i| {
                let angle = std::f64::consts::PI * i as f64 / (HOP - 1) as f64;
                0.5 - 0.5 * angle.cos()
            }),
        }
    }

    pub(super) fn reset(&mut self) {
        self.input.fill([0.0; FFT]);
        self.head = 0;

        for history in &mut self.history {
            history.fill([Complex64::default(); BINS]);
        }
    }

    pub(super) fn input(&mut self, pos: usize, samples: [f64; 2]) {
        for (input, sample) in self.input.iter_mut().zip(samples) {
            input[HOP + pos] = sample;
        }
    }

    pub(super) fn partition(
        &mut self,
        kernel: &mut Kernel,
        lane: usize,
        offset: usize,
        taps: &[f64],
    ) {
        self.work[..taps.len()].copy_from_slice(taps);
        self.work[taps.len()..].fill(0.0);
        self.plans
            .forward
            .process_with_scratch(
                &mut self.work,
                &mut kernel.partitions[lane][offset],
                &mut self.scratch,
            )
            .unwrap();
    }

    pub(super) fn push(&mut self) {
        self.head = (self.head + 1) % PARTITIONS;

        for (input, history) in self.input.iter_mut().zip(&mut self.history) {
            self.work.copy_from_slice(input);
            self.plans
                .forward
                .process_with_scratch(&mut self.work, &mut history[self.head], &mut self.scratch)
                .unwrap();
            input.copy_within(HOP.., 0);
        }
    }

    pub(super) fn render(
        &mut self,
        current: &Kernel,
        next: Option<&Kernel>,
        count: usize,
        output: &mut [[f64; HOP]; 4],
    ) {
        for (lane, output) in output.iter_mut().enumerate() {
            self.render_channel(current, count, lane);

            for (output, &value) in output.iter_mut().zip(&self.work[HOP..]) {
                *output = value / FFT as f64;
            }

            if let Some(next) = next {
                self.render_channel(next, count, lane);

                for ((output, &value), &fade) in
                    output.iter_mut().zip(&self.work[HOP..]).zip(&self.fade)
                {
                    *output += fade * (value / FFT as f64 - *output);
                }
            }
        }
    }

    fn render_channel(&mut self, kernel: &Kernel, count: usize, lane: usize) {
        if kernel.identity && lane < 2 {
            let head = (self.head + PARTITIONS - count / 2) % PARTITIONS;
            self.bins.copy_from_slice(&self.history[lane][head]);
            self.inverse();
            return;
        }

        let routes = &kernel.routes[2 * lane..2 * lane + 2];
        if routes.iter().all(Option::is_none) {
            self.work[HOP..].fill(0.0);
            return;
        }

        self.bins.fill(Complex64::default());

        for (history, route) in self.history.iter().zip(routes) {
            let Some(route) = route else { continue };
            let filters = &kernel.partitions[*route][..count + usize::from(kernel.last_tap)];
            let history = history[..=self.head]
                .iter()
                .rev()
                .chain(history[self.head + 1..].iter().rev());

            for (filter, input) in filters.iter().zip(history) {
                for ((bin, &gain), &sample) in self.bins.iter_mut().zip(filter).zip(input) {
                    *bin += gain * sample;
                }
            }
        }

        self.inverse();
    }

    fn inverse(&mut self) {
        self.plans
            .inverse
            .process_with_scratch(&mut self.bins, &mut self.work, &mut self.scratch)
            .unwrap();
    }
}
