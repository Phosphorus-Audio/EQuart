use super::super::REGION_COUNT;
use crate::precision::Sample;
use realfft::num_complex::Complex32;

pub(super) const STEP: usize = 8;
const EDGES: usize = REGION_COUNT * 4;
pub(super) type Gains = [[f32; 2]; REGION_COUNT];

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Shape {
    pub low: f32,
    pub high: f32,
    pub softness: f32,
}

impl Shape {
    fn edges(self, rate: f32) -> [f32; 4] {
        let nyquist = rate * 0.5;
        let low = self.low.min(nyquist * 0.98);
        let high = self.high.min(nyquist * 0.98);
        let width = (high / low).sqrt();
        let spread = 2.0_f32.powf(self.softness * 0.005).min(width);
        let bottom = if low <= crate::analyzer::MIN_HZ {
            [0.0; 2]
        } else {
            [low / spread, low * spread]
        };
        let top = if self.high >= nyquist * 0.98 {
            [nyquist; 2]
        } else {
            [high / spread, high * spread]
        };
        [bottom[0], bottom[1], top[0], top[1].min(nyquist)]
    }
}

type Weights = [f32; REGION_COUNT];

fn weights(edges: &[[f32; 4]; REGION_COUNT], duplicates: &Weights, hz: f32) -> Weights {
    let mut result = std::array::from_fn(|index| {
        let [lo, start, stop, hi] = edges[index];
        let weight = if hz >= start && hz < stop {
            1.0
        } else if (hz >= lo && hz < start) || (hz >= stop && hz < hi) {
            0.5
        } else {
            0.0
        };
        weight / duplicates[index]
    });
    let sum = result.iter().sum::<f32>().max(1.0);
    result.iter_mut().for_each(|weight| *weight /= sum);
    result
}

#[derive(Clone, Copy, Default)]
struct Mix {
    weights: Weights,
    target_weights: Weights,
    value: [f64; 2],
    step: [f64; 2],
}

impl Mix {
    fn transition(&mut self, fraction: f32) {
        for (weight, target) in self.weights.iter_mut().zip(self.target_weights) {
            *weight += fraction * (target - *weight);
        }
    }

    fn target(&mut self, gains: &Gains) {
        let mut next = [0.0; 2];
        for (&weight, gain) in self.weights.iter().zip(gains) {
            next[0] += f64::from(weight * gain[0]);
            next[1] += f64::from(weight * gain[1]);
        }

        self.step = std::array::from_fn(|ch| (next[ch] - self.value[ch]) / STEP as f64);
    }

    fn tick(&mut self) -> [f64; 2] {
        for (value, step) in self.value.iter_mut().zip(self.step) {
            *value += step;
        }
        self.value
    }
}

#[derive(Clone, Copy, Default)]
struct Edge {
    g: f64,
    target: f64,
    state: [f64; 2],
    mix: Mix,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct MaskResponse {
    pub(crate) high: [f32; 2],
    pub(crate) edges: [(f32, [f32; 2]); EDGES],
}

impl MaskResponse {
    pub(crate) fn active(&self) -> bool {
        self.high != [0.0; 2] || self.edges.iter().any(|&(_, gain)| gain != [0.0; 2])
    }

    pub(crate) fn db(&self, hz: f32, rate: f32) -> f32 {
        let delay = Complex32::from_polar(1.0, -std::f32::consts::TAU * hz / rate);
        let one = Complex32::new(1.0, 0.0);
        let mut stereo = self.high.map(|delta| one + delta);

        for &(g, delta) in &self.edges {
            if g != 0.0 && delta != [0.0; 2] {
                // this is the same trapezoidal lowpass used by the audio mask
                let low = g * (one + delay) / (one + (2.0 * g - 1.0) * delay);
                for (response, gain) in stereo.iter_mut().zip(delta) {
                    *response += low * gain;
                }
            }
        }

        let power = (stereo[0].norm_sqr() + stereo[1].norm_sqr()) * 0.5;
        10.0 * power.max(1e-12).log10()
    }
}

#[derive(Default)]
pub(super) struct Mask {
    key: [Option<Shape>; REGION_COUNT],
    rate: f32,
    alpha: f64,
    edges: [Edge; EDGES],
    high: Mix,
    phase: usize,
    transition: usize,
}

impl Mask {
    pub(super) fn response(&self) -> MaskResponse {
        MaskResponse {
            high: self.high.value.map(|value| value as f32),
            edges: self
                .edges
                .map(|edge| (edge.g as f32, edge.mix.value.map(|value| value as f32))),
        }
    }

    pub(super) fn configure(&mut self, shape: [Option<Shape>; REGION_COUNT], rate: f32) {
        if self.key == shape && self.rate == rate {
            return;
        }

        let nyquist = rate * 0.5;
        let edges = shape.map(|shape| shape.map_or([0.0; 4], |shape| shape.edges(rate)));
        let duplicates =
            edges.map(|edge| edges.iter().filter(|&&candidate| candidate == edge).count() as f32);
        let mut sorted: [(usize, f32); EDGES] = std::array::from_fn(|i| (i, edges[i / 4][i % 4]));
        sorted.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));

        for edge in &mut self.edges {
            edge.mix.target_weights = [0.0; REGION_COUNT];
        }

        let mut last = 0.0;
        let mut before = weights(&edges, &duplicates, 0.0);
        let mut i = 0;

        while i < sorted.len() {
            let (slot, hz) = sorted[i];
            i += 1;
            if hz <= last || hz >= nyquist {
                continue;
            }

            let following = sorted[i..]
                .iter()
                .find(|next| next.1 > hz)
                .map_or(nyquist, |next| next.1);
            let after = weights(&edges, &duplicates, (hz + following) * 0.5);
            let edge = &mut self.edges[slot];
            let g = (std::f64::consts::PI * f64::from(hz / rate)).tan();
            edge.target = g / (1.0 + g);
            if edge.g == 0.0 {
                edge.g = edge.target;
            }
            // neighboring bands share this lowpass edge with opposite signs
            edge.mix.target_weights = std::array::from_fn(|i| before[i] - after[i]);
            before = after;
            last = hz;
        }

        self.high.target_weights = before;
        let initial = self.rate == 0.0;
        self.transition = if initial {
            1
        } else {
            (rate * 0.006) as usize / STEP + 1
        };
        self.alpha = 1.0 - (-1.0 / (0.006 * f64::from(rate))).exp();
        self.key = shape;
        self.rate = rate;
    }

    pub(super) fn sample<T: Sample>(&mut self, input: [T; 2], gains: &Gains) -> [T; 2] {
        if self.phase == 0 {
            if self.transition != 0 {
                let fraction = 1.0 / self.transition as f32;
                self.high.transition(fraction);
                for edge in &mut self.edges {
                    edge.mix.transition(fraction);
                }
                self.transition -= 1;
            }

            self.high.target(gains);
            for edge in &mut self.edges {
                edge.mix.target(gains);
            }
        }
        self.phase = (self.phase + 1) % STEP;

        let high = self.high.tick();
        let input_f64 = input.map(Sample::double);
        let mut delta: [f64; 2] = std::array::from_fn(|ch| high[ch] * input_f64[ch]);

        for edge in self.edges.iter_mut().filter(|edge| edge.target != 0.0) {
            let mix = edge.mix.tick();
            edge.g += self.alpha * (edge.target - edge.g);

            for ch in 0..2 {
                let v = edge.g * (input_f64[ch] - edge.state[ch]);
                let low = edge.state[ch] + v;
                edge.state[ch] = low + v;
                delta[ch] += mix[ch] * low;
            }
        }

        std::array::from_fn(|ch| {
            if delta[ch] == 0.0 {
                input[ch]
            } else {
                input[ch] + T::value(delta[ch])
            }
        })
    }
}
