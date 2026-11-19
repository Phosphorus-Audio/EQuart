# Dynamic modifier

Pick a band with Gain, click `+` -> **Dynamic** -> **Add** (or double-click it). Controls live in the band panel (scroll horizontally to see all). Power icon bypasses it while keeping settings; **Remove modifier** detaches it without losing values.

## Detection

- **Detect** and **Relative** read Main or Sidechain (power-averaged RMS, full-scale sine is ~ -3.01 dBFS).
- **Follow**: unity-peak bandpass at band freq & Q. **Free**: custom freq & Q. **Broadband**: full input. **Band shape**: legacy preset detector.
- Missing Sidechain in host just releases gain offset to 0.

## Level & gain

- **Relative Amount**: 0% = Detect RMS; 100% = -24 dB + power density diff (`power / ENBW`) between Detect and Relative.
- **Onsets**: positive diff between 5 ms and 45 ms envelopes, blended via **Onsets Amount** before Threshold.
- **Floor**: -120 dB so silence stays finite.
- **Above / Below**: target = signed min of `abs(Range)` and `abs(level - Threshold) * Ratio / 100`.
- **Attack / Release**: smooths gain target at coeff clock, then EQ smoothing applies it to biquads.
