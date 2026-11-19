//! RBJ cookbook biquads for tests (kept separate from filter.rs)

#![cfg(test)]

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Biquad {
    fn normalize(a0: f64, a1: f64, a2: f64, b0: f64, b1: f64, b2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    pub fn peak(fs: f64, freq: f64, gain: f64, q: f64) -> Self {
        let omega = 2.0 * PI * freq / fs;
        let sn = omega.sin();
        let cs = omega.cos();
        let ampl = 10.0_f64.powf(gain / 40.0);
        let alpha = sn / (2.0 * q);

        Self::normalize(
            1.0 + alpha / ampl,
            -2.0 * cs,
            1.0 - alpha / ampl,
            1.0 + alpha * ampl,
            -2.0 * cs,
            1.0 - alpha * ampl,
        )
    }

    pub fn low_shelf(fs: f64, freq: f64, gain: f64, q: f64) -> Self {
        let omega = 2.0 * PI * freq / fs;
        let sn = omega.sin();
        let cs = omega.cos();
        let ampl = 10.0_f64.powf(gain / 40.0);
        let beta = sn * ampl.sqrt() / q;

        Self::normalize(
            (ampl + 1.0) + (ampl - 1.0) * cs + beta,
            -2.0 * ((ampl - 1.0) + (ampl + 1.0) * cs),
            (ampl + 1.0) + (ampl - 1.0) * cs - beta,
            ampl * ((ampl + 1.0) - (ampl - 1.0) * cs + beta),
            2.0 * ampl * ((ampl - 1.0) - (ampl + 1.0) * cs),
            ampl * ((ampl + 1.0) - (ampl - 1.0) * cs - beta),
        )
    }

    pub fn high_shelf(fs: f64, freq: f64, gain: f64, q: f64) -> Self {
        let omega = 2.0 * PI * freq / fs;
        let sn = omega.sin();
        let cs = omega.cos();
        let ampl = 10.0_f64.powf(gain / 40.0);
        let beta = sn * ampl.sqrt() / q;

        Self::normalize(
            (ampl + 1.0) - (ampl - 1.0) * cs + beta,
            2.0 * ((ampl - 1.0) - (ampl + 1.0) * cs),
            (ampl + 1.0) - (ampl - 1.0) * cs - beta,
            ampl * ((ampl + 1.0) + (ampl - 1.0) * cs + beta),
            -2.0 * ampl * ((ampl - 1.0) + (ampl + 1.0) * cs),
            ampl * ((ampl + 1.0) + (ampl - 1.0) * cs - beta),
        )
    }

    pub fn low_pass(fs: f64, freq: f64, q: f64) -> Self {
        let omega = 2.0 * PI * freq / fs;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * q);

        Self::normalize(
            1.0 + alpha,
            -2.0 * cs,
            1.0 - alpha,
            (1.0 - cs) / 2.0,
            1.0 - cs,
            (1.0 - cs) / 2.0,
        )
    }

    pub fn high_pass(fs: f64, freq: f64, q: f64) -> Self {
        let omega = 2.0 * PI * freq / fs;
        let sn = omega.sin();
        let cs = omega.cos();
        let alpha = sn / (2.0 * q);

        Self::normalize(
            1.0 + alpha,
            -2.0 * cs,
            1.0 - alpha,
            (1.0 + cs) / 2.0,
            -(1.0 + cs),
            (1.0 + cs) / 2.0,
        )
    }

    /// Magnitude response in dB at `hz`, evaluated on the unit circle.
    pub fn magnitude_db(&self, fs: f64, hz: f64) -> f64 {
        let w = 2.0 * PI * hz / fs;
        let (sin1, cos1) = (w.sin(), w.cos());
        let (sin2, cos2) = ((2.0 * w).sin(), (2.0 * w).cos());

        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = -(self.b1 * sin1 + self.b2 * sin2);
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = -(self.a1 * sin1 + self.a2 * sin2);

        let num = (num_re * num_re + num_im * num_im).sqrt();
        let den = (den_re * den_re + den_im * den_im).sqrt().max(1e-18);

        20.0 * (num / den).max(1e-18).log10()
    }
}
