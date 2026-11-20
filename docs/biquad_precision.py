"""
Compare scalar bell-filter realizations with a 64-digit Decimal reference
a test for equart

Run with Python 3 and NumPy
just: python docs/biquad_precision.py
"""

from decimal import Decimal, localcontext
from math import fsum, inf, log10, sin, tau

import numpy as np


RATE = 48_000
MEASURE = 12_000
AMPLITUDE = 10 ** (-6.34 / 20)
PI = Decimal("3.141592653589793238462643383279502884197169399375105820974944592307816406286")


def tangent(x):
    sine_term, cosine_term = x, Decimal(1)
    sine, cosine = sine_term, cosine_term
    for order in range(1, 45):
        sine_term *= -x * x / ((2 * order) * (2 * order + 1))
        cosine_term *= -x * x / ((2 * order - 1) * (2 * order))
        sine += sine_term
        cosine += cosine_term
    return sine / cosine


def design(frequency, q, db):
    # The same bilinear-transformed analog bell for all three realizations.
    g = tangent(PI * frequency / RATE)
    a = Decimal(10) ** (db / 40)
    k = 1 / (q * a)
    h = 1 / (1 + g * (g + k))
    a1 = 2 * (g * g - 1) * h
    a2 = (1 - k * g + g * g) * h
    mix = k * (a * a - 1)
    b0 = 1 + h * g * mix
    b2 = a2 - h * g * mix
    return (b0, a1, b2, a1, a2), (h, h * g, h * g * g, mix)


def df_i(samples, coeff, zero):
    b0, b1, b2, a1, a2 = coeff
    x1 = x2 = y1 = y2 = zero
    result = []
    for x in samples:
        y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        result.append(y)
        x1, x2, y1, y2 = x, x1, y, y1
    return result


def tdf_ii(samples, coeff, zero):
    b0, b1, b2, a1, a2 = coeff
    s1 = s2 = zero
    result = []
    for x in samples:
        y = b0 * x + s1
        s1 = b1 * x - a1 * y + s2
        s2 = b2 * x - a2 * y
        result.append(y)
    return result


def tpt_svf(samples, coeff, zero):
    # Same scalar recurrence and operation order as src/dsp.rs for a bell.
    a1, a2, a3, mix = coeff
    s1 = s2 = zero
    result = []
    for x in samples:
        v3 = x - s2
        v1 = a1 * s1 + a2 * v3
        v2 = s2 + a2 * s1 + a3 * v3
        s1 = v1 + v1 - s1
        s2 = v2 + v2 - s2
        result.append(x + mix * v1)
    return result


def cascade(samples, coefficients, run, zero, warmup):
    for section in coefficients:
        samples = run(samples, section, zero)
    return samples[warmup:]


def residual_db(actual, reference, power):
    error = fsum(float(a - b) ** 2 for a, b in zip(actual, reference, strict=True))
    return 10 * log10(error / power) if error else -inf


def decimal_samples(samples):
    return [Decimal.from_float(float(sample)) for sample in samples]


def cases():
    for hz in (24, 48, 96):
        yield f"{hz} Hz, Q=16, +12 dB", hz, [(hz, "16", "12")]
    for hz in (10_000, 15_000, 20_000):
        yield f"{hz} Hz, Q=2, +24 dB", hz, [(hz, "2", "24")]
    yield "32 bells, 1.5 kHz tone", 1500, [
        (200 + 450 * i, "1.2", "0.5" if i % 2 == 0 else "-0.5") for i in range(32)
    ]
    for db in ("0.001", "-0.001", "0.000001"):
        yield f"1 kHz, Q=1, {db} dB", 1000, [(1000, "1", db)]


def compare(label, tone, sections, dtype):
    # Let the slowest high-Q pole settle before starting the RMS window.
    warmup = 6 * RATE if tone < 100 else MEASURE
    source = [dtype(np.float32(AMPLITUDE * sin(tau * ((tone * i) % RATE) / RATE)))
              for i in range(warmup + MEASURE)]
    reference_input = decimal_samples(source)
    df, svf = zip(*(design(f, Decimal(q), Decimal(db)) for f, q, db in sections), strict=True)
    ref = cascade(reference_input, df, df_i, Decimal(0), warmup)
    check = cascade(reference_input, svf, tpt_svf, Decimal(0), warmup)
    power = fsum(float(x) ** 2 for x in ref)
    assert residual_db(check, ref, power) < -1000, label
    for name, run, design_coeffs in (
        ("DF-I", df_i, df), ("TDF-II", tdf_ii, df), ("TPT-SVF", tpt_svf, svf)
    ):
        rounded = [tuple(dtype(float(c)) for c in section) for section in design_coeffs]
        rounded_ref = cascade(reference_input, [decimal_samples(c) for c in rounded], run, Decimal(0), warmup)
        actual = decimal_samples(cascade(source, rounded, run, dtype(0), warmup))
        total = residual_db(actual, ref, power)
        coefficients = residual_db(rounded_ref, ref, power)
        arithmetic = residual_db(actual, rounded_ref, power)
        print(f'"{label}",{dtype.__name__},{name},{total:.2f},{coefficients:.2f},{arithmetic:.2f}', flush=True)


def main():
    print("case,precision,topology,total_dbr,coefficients_dbr,arithmetic_dbr", flush=True)
    with localcontext(prec=64):
        for case in cases():
            for dtype in (np.float32, np.float64):
                compare(*case, dtype)


if __name__ == "__main__":
    main()
