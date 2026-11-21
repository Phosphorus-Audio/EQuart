# filter maths

Notes on what `filter.rs` uses

## processing modes

- **Zero Latency** runs the TPT-SVF at the host rate and reports zero samples of latency.
- **Natural Phase** runs the TPT-SVF at 2× or 4× the host rate to reduce high-frequency warping. The resampling FIR adds 32 or 48 samples of latency. Auto selects 4×.
- **Linear Phase** uses a symmetric FIR and 256-sample partitioned overlap-save. At 44.1 and 48 kHz, Short, Standard, High and Extended use 1,025, 4,097, 8,193 and 32,769 taps. They report 768, 2,304, 4,352 and 16,640 samples of latency, including the 256-sample convolution hop.
- **High sample rates** at or above 64 kHz double the FIR length for all Linear Phase tiers. At 96 kHz, the resulting PDC is 1,280, 4,352, 8,448 and 33,024 samples. The design scales in one step and caps at twice the 48 kHz length, so filters at still higher rates gradually lose equivalent low-frequency resolution.

The FIR design samples the magnitude of the routed EQ cascade on a uniform grid. Eight-point frequency-cell integration is used for the first 32 cells when a narrow pole is under-resolved there, with a transition over the last eight cells. This reduces sensitivity to a narrow resonance landing on a grid point. The inverse FFT is normalized by its length, mirrored about the center tap, and tapered over the outer quarter of each half with a raised cosine. The sinc correction partially compensates for the cell integration. The final tap is exactly zero and can be omitted from convolution.

Linear Phase records filter controls every 256 samples. Short and Standard can renew a kernel every hop at base rates for a cascade with at most eight filter sections. Larger cascades use two hops, while High and Extended use four hops. The hop count doubles in the doubled FIR tier. Design work is spread across input chunks. A raised-cosine crossfade over 256 samples evaluates both symmetric kernels on the same input history. Each frozen kernel has linear phase apart from the sign of its real response. A moving gain or filter creates a time-varying system.

The plot uses a snapshot of the last applied stereo FIR, using RMS magnitude across the left and right paths for independent stereo inputs. The audio thread publishes a requested snapshot of the half taps and the editor evaluates it. The combined line represents the EQ FIR before mix, output trim and delta audition, while individual band curves remain controls for the desired response. The phase overlay removes the reported delay. Internal bypass fades to aligned dry audio over 5 ms. Changing quality or processing mode resets convolution history and updates host PDC.

The absolute band-pass and high-pass power detectors shorten smoothing at short attack settings when the detector frequency allows it. Low-frequency, wideband, relative and onset detection retain their established smoothing. Attack is the gain smoother’s time constant. Detector filtering, FIR scheduling and crossfades also affect the measured step response.

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

Native VST3 processing supports both 32-bit and 64-bit host audio. And we should use f64 so...

Run `python docs/biquad_precision.py` to compare DF-I, TDF-II and TPT-SVF against a
64-decimal-digit reference using the same quantized input. For the production AVX2/FMA kernel,
run `cargo test avx2_residual_against_f64_svf -- --nocapture` on a supported CPU.

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
