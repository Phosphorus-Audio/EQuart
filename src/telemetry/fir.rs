use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence};

use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex64};

use crate::{engine::Engine, processing_mode::LinearQuality};

pub(crate) struct FirTelemetry {
    requested: AtomicBool,
    published_revision: AtomicU64,
    epoch: AtomicU32,
    rate: AtomicU32,
    count: AtomicU32,
    routes: AtomicU32,
    taps: [Box<[AtomicU64]>; 4],
}

impl Default for FirTelemetry {
    fn default() -> Self {
        Self {
            requested: AtomicBool::new(false),
            published_revision: AtomicU64::new(0),
            epoch: AtomicU32::new(0),
            rate: AtomicU32::new(0),
            count: AtomicU32::new(0),
            routes: AtomicU32::new(0),
            taps: std::array::from_fn(|_| {
                (0..=LinearQuality::MAX_CENTER)
                    .map(|_| AtomicU64::new(0))
                    .collect()
            }),
        }
    }
}

impl FirTelemetry {
    pub(super) fn publish(&self, engine: &Engine) {
        if !self.requested.swap(false, Ordering::Relaxed) {
            return;
        }

        let revision = engine.linear_response_revision();
        if revision == self.published_revision.load(Ordering::Relaxed) {
            return;
        }
        self.published_revision.store(revision, Ordering::Relaxed);

        self.epoch.fetch_add(1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.rate.store(engine.rate.to_bits(), Ordering::Relaxed);

        let mut count = 0;
        let mut routes = 0_u32;
        if let Some(response) = engine.linear_response() {
            for (i, taps) in response.iter().enumerate() {
                let Some(taps) = taps else { continue };
                count = taps.len();
                let previous = response[..i]
                    .iter()
                    .position(|other| other.is_some_and(|other| std::ptr::eq(other, *taps)));
                let source = previous.unwrap_or(i);
                routes |= (source as u32 + 1) << (i * 4);

                if previous.is_none() {
                    for (slot, tap) in self.taps[i].iter().zip(*taps) {
                        slot.store(tap.to_bits(), Ordering::Relaxed);
                    }
                }
            }
        }

        self.count.store(count as u32, Ordering::Relaxed);
        self.routes.store(routes, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release);
    }
}

struct ResponseFft {
    plan: Arc<dyn RealToComplex<f64>>,
    time: Vec<f64>,
    spectrum: Vec<Complex64>,
    scratch: Vec<Complex64>,
}

impl ResponseFft {
    fn new(center: usize) -> Self {
        let plan = RealFftPlanner::new().plan_fft_forward(16 * center);
        Self {
            time: plan.make_input_vec(),
            spectrum: plan.make_output_vec(),
            scratch: plan.make_scratch_vec(),
            plan,
        }
    }

    fn accumulate(&mut self, taps: &[f64], power: &mut [f64], scale: f64) {
        self.time.fill(0.0);
        self.time[..taps.len()].copy_from_slice(taps);
        let fft = self.time.len();

        for (lag, &tap) in taps.iter().enumerate().skip(1) {
            self.time[fft - lag] = tap;
        }

        self.plan
            .process_with_scratch(&mut self.time, &mut self.spectrum, &mut self.scratch)
            .unwrap();
        for (power, response) in power.iter_mut().zip(&self.spectrum) {
            *power += scale * response.re * response.re;
        }
    }
}

#[derive(Default)]
pub(crate) struct FirResponse {
    epoch: u32,
    rate: f32,
    count: usize,
    center: usize,
    taps: [Vec<f64>; 4],
    previous: [Vec<f64>; 4],
    previous_routes: u32,
    fft: Option<ResponseFft>,
    power: Vec<f64>,
}

impl FirResponse {
    pub(crate) fn update(&mut self, source: &FirTelemetry) {
        let epoch = source.epoch.load(Ordering::Acquire);
        if epoch != self.epoch && epoch & 1 == 0 {
            self.read(source, epoch);
        }

        source.requested.store(true, Ordering::Relaxed);
    }

    fn read(&mut self, source: &FirTelemetry, epoch: u32) {
        let count = source.count.load(Ordering::Relaxed) as usize;
        let rate = f32::from_bits(source.rate.load(Ordering::Relaxed));
        let routes = source.routes.load(Ordering::Relaxed);
        let mut usage = [0; 4];

        for lane in 0..4 {
            let tap = (routes >> (4 * lane)) & 15;
            if tap != 0 {
                usage[tap as usize - 1] += 1;
            }
        }

        for (i, taps) in self.taps.iter_mut().enumerate() {
            taps.clear();

            if usage[i] != 0 {
                taps.extend(
                    source.taps[i][..count]
                        .iter()
                        .map(|tap| f64::from_bits(tap.load(Ordering::Relaxed))),
                );
            }
        }

        fence(Ordering::Acquire);
        if source.epoch.load(Ordering::Relaxed) != epoch {
            return;
        }

        self.epoch = epoch;
        self.rate = rate;
        self.count = count;

        if count == 0 || (self.taps == self.previous && routes == self.previous_routes) {
            return;
        }

        let center = count - 1;
        if self.fft.is_none() || self.center != center {
            self.fft = Some(ResponseFft::new(center));
            self.center = center;
        }

        let fft = self.fft.as_mut().unwrap();
        self.power.resize(fft.spectrum.len(), 0.0);
        self.power.fill(0.0);

        for (taps, usage) in self.taps.iter().zip(usage) {
            if usage > 0 {
                fft.accumulate(taps, &mut self.power, 0.5 * f64::from(usage));
            }
        }

        std::mem::swap(&mut self.taps, &mut self.previous);
        self.previous_routes = routes;
    }

    pub(crate) fn db(&self, hz: f32, rate: f32, center: usize) -> Option<f32> {
        if self.count != center + 1 || self.rate != rate {
            return None;
        }

        let bin = f64::from(hz.clamp(0.0, rate * 0.5) / rate) * (16 * center) as f64;
        let index = bin as usize;
        let a = self.power[index];
        let b = self.power.get(index + 1).copied().unwrap_or(a);
        let power = a + (b - a) * bin.fract();
        Some((10.0 * power.max(1e-18).log10()) as f32)
    }
}
