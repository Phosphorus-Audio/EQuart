use super::{Coefficients, FilterKind};

pub(crate) const SECTIONS: usize = 7;
const CUTOFF: f64 = 1.0068398556146663;
const PASS_GAIN: f64 = 0.9942600739529567;

// scipy.signal.ellip(14, 0.05, 96, 1, analog=True, output="zpk")
// each triple holds the pole radius, positive damping and imaginary zero
// use low-q sections first and pair the last poles with their nearest zeros
const PROTOTYPE: [(f64, f64, f64); SECTIONS] = [
    (0.37699665812001881, 1.7264893401077277, 6.2228875998382716),
    (0.58132812415848933, 0.88380400148035321, 2.2268630167943915),
    (0.76750396940359888, 0.43972881742720671, 1.5117098668353401),
    (0.88786683640700204, 0.22119900422060684, 1.2562072285446062),
    (0.95588527320527661, 0.10972682001278135, 1.1460106954421438),
    (
        0.99072199756064838,
        0.050262194507619908,
        1.0963726699134451,
    ),
    (1.0051894025074892, 0.014482491400398244, 1.0769132253073561),
];

pub(crate) fn design(rate: f64, hz: f64, kind: FilterKind) -> [Coefficients<f64>; SECTIONS] {
    let warp = (std::f64::consts::PI * hz / rate).tan();
    let highpass = kind == FilterKind::Highpass;

    std::array::from_fn(|i| {
        let (pole, damping, zero) = PROTOTYPE[i];
        let ratio = (zero / pole).powi(2);
        let (g, direct, constant) = if highpass {
            (warp * CUTOFF / pole, 1.0, ratio.recip())
        } else {
            (warp / CUTOFF * pole, ratio.recip(), 1.0)
        };
        let gain = if i == 0 { PASS_GAIN } else { 1.0 };

        // realize the analog pole and zero pair in the bilinear TPT state space
        Coefficients::new(
            g,
            damping,
            [
                gain * direct,
                -(gain * direct) * damping,
                gain * constant - gain * direct,
            ],
        )
    })
}
