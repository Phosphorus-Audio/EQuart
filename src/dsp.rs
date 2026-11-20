//! Block SIMD kernel for static TPT SVF sections (unrolled affine recurrence, falls back to step() when moving)

use crate::filter::Coefficients;
use crate::precision::Sample;
use std::sync::atomic::{AtomicU8, Ordering};

/// Frames per SIMD block. Must match the lane count of the widest kernel.
pub const BLOCK: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State<T = f32> {
    pub ic1: T,
    pub ic2: T,
}

impl<T: Sample> State<T> {
    pub(crate) fn from_state<S: Sample>(state: State<S>) -> Self {
        Self {
            ic1: T::value(state.ic1.double()),
            ic2: T::value(state.ic2.double()),
        }
    }

    pub(crate) fn zero() -> Self {
        Self {
            ic1: T::zero(),
            ic2: T::zero(),
        }
    }
}

/// One sample through one section.
#[inline(always)]
pub fn step<T: Sample>(state: &mut State<T>, c: &Coefficients<T>, x: T) -> T {
    let v3 = x - state.ic2;
    let v1 = c.a1 * state.ic1 + c.a2 * v3;
    let v2 = state.ic2 + c.a2 * state.ic1 + c.a3 * v3;

    state.ic1 = T::value(2.0) * v1 - state.ic1;
    state.ic2 = T::value(2.0) * v2 - state.ic2;

    c.mix[0] * x + c.mix[1] * v1 + c.mix[2] * v2
}

/// Affine state-space form of one section.
#[derive(Clone, Copy, Debug)]
struct Affine {
    /// `M`, row major.
    m: [f32; 4],
    /// `N`.
    n: [f32; 2],
    /// `c`.
    c: [f32; 2],
    /// `d`.
    d: f32,
}

impl Affine {
    fn new(coeff: &Coefficients) -> Self {
        let (a1, a2, a3) = (coeff.a1, coeff.a2, coeff.a3);
        let [m0, m1, m2] = coeff.mix;

        Self {
            m: [2.0 * a1 - 1.0, -2.0 * a2, 2.0 * a2, 1.0 - 2.0 * a3],
            n: [2.0 * a2, 2.0 * a3],
            c: [m1 * a1 + m2 * a2, m2 * (1.0 - a3) - m1 * a2],
            d: m0 + m1 * a2 + m2 * a3,
        }
    }

    #[inline(always)]
    fn mul(&self, v: [f32; 2]) -> [f32; 2] {
        [
            self.m[0] * v[0] + self.m[1] * v[1],
            self.m[2] * v[0] + self.m[3] * v[1],
        ]
    }

    #[inline(always)]
    fn dot(&self, v: [f32; 2]) -> f32 {
        self.c[0] * v[0] + self.c[1] * v[1]
    }
}

/// Block form of one section, rebuilt whenever its coefficients move.
#[derive(Clone, Copy, Debug)]
pub struct Kernel {
    /// Impulse response, `h[0] = d`.
    h: [f32; BLOCK],
    /// `c·M^k`, split by state component, for `k = 0..BLOCK`.
    ca1: [f32; BLOCK],
    ca2: [f32; BLOCK],
    /// `M^(BLOCK-1-j)·N`, split by state component.
    u1: [f32; BLOCK],
    u2: [f32; BLOCK],
    /// `M^BLOCK`.
    power: [f32; 4],
    /// Kept for the per-sample tail, which has no block form.
    coeff: Coefficients,
    /// Rejected when the state transition leaves the unit circle.
    stable: bool,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new(&Coefficients::identity())
    }
}

impl Kernel {
    pub fn new(coeff: &Coefficients) -> Self {
        let affine = Affine::new(coeff);

        // h[0] = d, h[m] = c·M^(m-1)·N.
        let mut h = [0.0; BLOCK];
        h[0] = affine.d;
        let mut v = affine.n;
        for slot in h.iter_mut().skip(1) {
            *slot = affine.dot(v);
            v = affine.mul(v);
        }

        // c·M^k.
        let mut ca1 = [0.0; BLOCK];
        let mut ca2 = [0.0; BLOCK];
        let mut row = affine.c;
        for k in 0..BLOCK {
            ca1[k] = row[0];
            ca2[k] = row[1];
            row = [
                row[0] * affine.m[0] + row[1] * affine.m[2],
                row[0] * affine.m[1] + row[1] * affine.m[3],
            ];
        }

        // M^(BLOCK-1-j)*N from back to front, leaves M^BLOCK in ak
        let mut u1 = [0.0; BLOCK];
        let mut u2 = [0.0; BLOCK];
        let mut w = affine.n;
        let mut ak = [1.0, 0.0, 0.0, 1.0];
        for j in (0..BLOCK).rev() {
            u1[j] = w[0];
            u2[j] = w[1];
            w = affine.mul(w);
            ak = mul_matrix(ak, affine.m);
        }

        let power = ak;

        // reject poles outside unit circle so block form won't diverge
        let trace = power[0] + power[3];
        let det = power[0] * power[3] - power[1] * power[2];
        let stable = power.iter().all(|v| v.is_finite())
            && det.is_finite()
            && det.abs() <= 1.0 + 1e-4
            && trace.is_finite()
            && (trace.abs() <= 2.0 + 1e-3);

        Self {
            h,
            ca1,
            ca2,
            u1,
            u2,
            power,
            coeff: *coeff,
            stable,
        }
    }

    /// Filter a buffer in place, `BLOCK` samples at a time.
    #[inline]
    pub fn apply_slice(&self, state: &mut State, buf: &mut [f32]) {
        if self.stable {
            #[cfg(target_arch = "x86_64")]
            if wide() {
                // SAFETY: the feature was detected above.
                unsafe { self.apply_avx2(state, buf) };
                return;
            }
        }

        for sample in buf.iter_mut() {
            *sample = step(state, &self.coeff, *sample);
        }
    }

    /// Filter exactly one block in place.
    #[cfg(test)]
    #[inline]
    pub fn apply(&self, state: &mut State, buf: &mut [f32; BLOCK]) {
        self.apply_slice(state, buf);
    }

    /// # Safety
    /// The current CPU must support AVX2 and FMA.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2,fma")]
    unsafe fn apply_avx2(&self, state: &mut State, buf: &mut [f32]) {
        use std::arch::x86_64::*;

        // SAFETY: each coefficient array contains eight initialized f32 lanes.
        let (ca1, ca2, u1, u2) = unsafe {
            (
                _mm256_loadu_ps(self.ca1.as_ptr()),
                _mm256_loadu_ps(self.ca2.as_ptr()),
                _mm256_loadu_ps(self.u1.as_ptr()),
                _mm256_loadu_ps(self.u2.as_ptr()),
            )
        };

        let mut blocks = buf.chunks_exact_mut(BLOCK);
        for chunk in &mut blocks {
            let ptr = chunk.as_mut_ptr();
            // SAFETY: chunks_exact_mut supplies eight readable lanes.
            let x = unsafe { _mm256_loadu_ps(ptr) };

            // shift x in registers to avoid store-forwarding stalls
            let tap = |j: usize| {
                // SAFETY: each table row contains eight initialized i32 lanes.
                let (index, keep) = unsafe {
                    (
                        _mm256_loadu_si256(SHIFTS[j].as_ptr().cast()),
                        _mm256_loadu_si256(KEEPS[j].as_ptr().cast()),
                    )
                };
                _mm256_and_ps(
                    _mm256_permutevar8x32_ps(x, index),
                    _mm256_castsi256_ps(keep),
                )
            };

            // The state update only needs the input, so it runs alongside the
            // convolution rather than after it.
            // SAFETY: this kernel and reduce_pair require the same CPU features.
            let (next1, next2) = unsafe { reduce_pair(_mm256_mul_ps(u1, x), _mm256_mul_ps(u2, x)) };
            let (s1, s2) = (state.ic1, state.ic2);
            state.ic1 = next1 + self.power[0] * s1 + self.power[1] * s2;
            state.ic2 = next2 + self.power[2] * s1 + self.power[3] * s2;

            // Five accumulators keep the convolution from serialising on
            // itself; the two state corrections seed two of them, so they cost
            // no extra latency.
            let gain = |j: usize| _mm256_set1_ps(self.h[j]);

            let y0 = _mm256_fmadd_ps(ca1, _mm256_set1_ps(s1), _mm256_mul_ps(gain(0), tap(0)));
            let y1 = _mm256_fmadd_ps(ca2, _mm256_set1_ps(s2), _mm256_mul_ps(gain(1), tap(1)));
            let y2 = _mm256_fmadd_ps(gain(3), tap(3), _mm256_mul_ps(gain(2), tap(2)));
            let y3 = _mm256_fmadd_ps(gain(5), tap(5), _mm256_mul_ps(gain(4), tap(4)));
            let y4 = _mm256_fmadd_ps(gain(7), tap(7), _mm256_mul_ps(gain(6), tap(6)));

            let y = _mm256_add_ps(
                _mm256_add_ps(y0, y1),
                _mm256_add_ps(_mm256_add_ps(y2, y3), y4),
            );
            // SAFETY: ptr refers to the same eight writable lanes loaded above.
            unsafe { _mm256_storeu_ps(ptr, y) };
        }

        for sample in blocks.into_remainder() {
            *sample = step(state, &self.coeff, *sample);
        }
    }
}

fn mul_matrix(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
    ]
}

/// Lane index that moves `x[k-j]` into lane `k`; the wrapped lanes are masked
/// off by [`KEEPS`].
#[cfg(target_arch = "x86_64")]
static SHIFTS: [[i32; BLOCK]; BLOCK] = [
    [0, 1, 2, 3, 4, 5, 6, 7],
    [0, 0, 1, 2, 3, 4, 5, 6],
    [0, 0, 0, 1, 2, 3, 4, 5],
    [0, 0, 0, 0, 1, 2, 3, 4],
    [0, 0, 0, 0, 0, 1, 2, 3],
    [0, 0, 0, 0, 0, 0, 1, 2],
    [0, 0, 0, 0, 0, 0, 0, 1],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// Bitmask that clears the `j` lanes a shift wrapped around.
#[cfg(target_arch = "x86_64")]
static KEEPS: [[i32; BLOCK]; BLOCK] = [
    [-1, -1, -1, -1, -1, -1, -1, -1],
    [0, -1, -1, -1, -1, -1, -1, -1],
    [0, 0, -1, -1, -1, -1, -1, -1],
    [0, 0, 0, -1, -1, -1, -1, -1],
    [0, 0, 0, 0, -1, -1, -1, -1],
    [0, 0, 0, 0, 0, -1, -1, -1],
    [0, 0, 0, 0, 0, 0, -1, -1],
    [0, 0, 0, 0, 0, 0, 0, -1],
];

/// # Safety
/// The current CPU must support AVX2 and FMA.
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "avx2,fma")]
unsafe fn reduce_pair(a: std::arch::x86_64::__m256, b: std::arch::x86_64::__m256) -> (f32, f32) {
    use std::arch::x86_64::*;

    let pairs = _mm256_hadd_ps(a, b);
    let quads = _mm256_hadd_ps(pairs, pairs);
    let sum = _mm_add_ps(
        _mm256_castps256_ps128(quads),
        _mm256_extractf128_ps(quads, 1),
    );

    (
        _mm_cvtss_f32(sum),
        _mm_cvtss_f32(_mm_shuffle_ps(sum, sum, 0b01_01_01_01)),
    )
}

/// One-time AVX2 + FMA probe, cached because it runs on the audio thread.
fn wide() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        static CACHED: AtomicU8 = AtomicU8::new(0);
        match CACHED.load(Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => {
                let ok = std::arch::is_x86_feature_detected!("avx2")
                    && std::arch::is_x86_feature_detected!("fma");
                CACHED.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
                ok
            }
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// `buf *= gain`, eight lanes at a time.
pub fn scale(buf: &mut [f32], gain: f32) {
    #[cfg(target_arch = "x86_64")]
    if wide() {
        // SAFETY: the feature was detected above.
        unsafe { scale_avx2(buf, gain) };
        return;
    }

    for sample in buf {
        *sample *= gain;
    }
}

/// # Safety
/// The current CPU must support AVX2 and FMA.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn scale_avx2(buf: &mut [f32], gain: f32) {
    use std::arch::x86_64::*;

    let g = _mm256_set1_ps(gain);
    let mut chunks = buf.chunks_exact_mut(BLOCK);
    for chunk in &mut chunks {
        // SAFETY: each chunk contains eight initialized, writable f32 lanes.
        unsafe {
            let v = _mm256_loadu_ps(chunk.as_ptr());
            _mm256_storeu_ps(chunk.as_mut_ptr(), _mm256_mul_ps(v, g));
        }
    }
    for sample in chunks.into_remainder() {
        *sample *= gain;
    }
}

/// `out = dry + (wet - dry) * amount`.
pub fn blend<T: Sample>(dry: &[T], wet: &mut [T], amount: f32) {
    let amount = T::value(f64::from(amount));
    for (w, &d) in wet.iter_mut().zip(dry) {
        *w = d + (*w - d) * amount;
    }
}

/// Largest absolute value across both channels, `NaN` if anything is not finite.
pub fn peak(left: &[f32], right: &[f32]) -> f32 {
    let mut peak = 0.0_f32;
    for sample in left.iter().chain(right) {
        let magnitude = sample.abs();
        if magnitude > peak || !magnitude.is_finite() {
            peak = magnitude;
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FilterKind;

    fn random(len: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / 8_388_608.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn kernel_switch_preserves_state() {
        let coeff = Coefficients::for_kind(48_000.0, 1_200.0, 1.0, 8.0, FilterKind::Peak);
        let kernel = Kernel::new(&coeff);
        let input = random(1_024, 0xbeef);

        // Alternate between the block kernel and the per-sample kernel; the
        // state is shared, so the output must match a single-path run.
        let mut mixed_state = State::default();
        let mut mixed = Vec::new();
        for (block, chunk) in input.chunks_exact(BLOCK).enumerate() {
            let mut buf = [0.0; BLOCK];
            buf.copy_from_slice(chunk);
            if block % 3 == 0 {
                for sample in buf.iter_mut() {
                    *sample = step(&mut mixed_state, &coeff, *sample);
                }
            } else {
                kernel.apply(&mut mixed_state, &mut buf);
            }
            mixed.extend_from_slice(&buf);
        }

        let mut reference = State::default();
        let expected: Vec<f32> = input
            .iter()
            .map(|x| step(&mut reference, &coeff, *x))
            .collect();

        let worst = expected
            .iter()
            .zip(&mixed)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);

        assert!(worst < 1e-5, "path switching drifted {worst}");
    }
}
