# filter maths

Notes on what `filter.rs` uses

## references

- **camilladsp (`src/filters/biquad.rs`)**: standard RBJ cookbook biquads in `f64`. Used as our test baseline.
- **lsp-dsp-units (`ButterworthFilter.cpp`)**: analytic Butterworth poles `Q_i = 1 / (2·sin(π(2i + 1)/(2n)))` (+1 real pole for odd orders).
- **ZLEqualizer (`source/dsp/filter/`)**: Ivantsov SVF with warped `w²` to fix Nyquist cramping, spreads Q across all sections `(√2·Q)^(1/n)`.

## what we use

Cytomic TPT SVF (`g = tan(π f/f_s)`, `k = 1/Q`, `mix0·x + mix1·band + mix2·low`), stable under modulation:

| type | design | reference |
| --- | --- | --- |
| bell | `k/a`, `mix = [1, k(a²-1), 0]` | RBJ peaking |
| shelves | `g` scaled by `√a`, `mix = [1, k(a-1), a²-1]` | RBJ shelf |
| notch | `mix = [1, -k, 0]` | RBJ notch |
| cuts | Butterworth cascade, exact pole Qs | lsp `ButterworthFilter` |
| cut resonance | Q scales highest-Q section only by `√2·Q` | f0lock |

## measurements

Compared against `src/reference.rs` via `cargo test`:

- **Single-section vs RBJ**: `worst deviation from the RBJ reference: 0.0115 dB` (just f32 rounding in stopband).
- **Butterworth cascades** (all 9 slopes @ 48 kHz, 1 kHz): -3.01 dB at cutoff (neutral Q), ≤ 0.001 dB ripple, cutoff gain tracks `20·log10(Q)` within 0.1 dB.
- **Resonance distribution** (96 dB/oct @ 1 kHz): spreading Q like ZLEqualizer pulls peak below cutoff and boosts it with slope, so we only scale the highest-Q section:

| scheme | peak @ Q=1 | peak @ Q=4 |
| --- | --- | --- |
| highest-Q section only (current) | +1.39 dB @ 954 Hz | +11.97 dB @ 994 Hz |
| even `(√2·Q)^(1/n)` spread | +2.57 dB @ 891 Hz | +13.66 dB @ 948 Hz |
| spread + ZL taper | +2.19 dB @ 916 Hz | +12.86 dB @ 974 Hz |

## high-frequency warping

Standard bilinear transform cramps near Nyquist (+12 dB bell worst error over 4 octaves):

| centre | Q=1 | Q=4 |
| --- | --- | --- |
| 1 kHz | 0.02 dB | 0.01 dB |
| 6 kHz | 0.78 dB | 0.39 dB |
| 12 kHz | 3.79 dB | 1.25 dB |
| 16 kHz | 7.37 dB | 1.96 dB |

Rescaling `g` and `Q` only shaves ~30% at best (error is structural), and re-deriving ZLEqualizer's AGPL Ivantsov mapping from scratch is too heavy, so we shipped oversampling instead.

## oversampling (`src/oversample.rs`)

Runs biquads at 2x/4x using a 65-tap linear-phase FIR per stage (Kaiser beta 8.6, cutoff at 0.25x oversampled rate; 32 frames latency at 2x, 48 at 4x).

+12 dB bell @ 6 kHz, probed at 12 kHz:

| mode | measured at 12 kHz | error |
| --- | --- | --- |
| 1x | 2.74 dB | 1.23 dB |
| 2x | 3.66 dB | 0.30 dB |
| 4x | 3.89 dB | 0.08 dB |

Cost for 128 biquads: 106 ns/frame (1x), 175 ns (2x), 364 ns (4x).

### half-band trick

At 0.25x cutoff, every even tap except center is 0. Making the even phase a pure delay (center=0.5, odd taps sum to 0.5) drops even-phase cost and balances DC gain:

| phase | before | after |
| --- | --- | --- |
| even (33 taps, 32 zero) | 7.85 ns | 0.68 ns |
| odd (32 real taps) | 7.85 ns | 7.85 ns |
