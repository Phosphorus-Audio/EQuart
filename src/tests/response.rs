use super::*;
use crate::modulation::relation::Curve;
use crate::region::{
    FundamentalSource, RegionControl,
    response::{Response, Scope, Value, whole},
};

fn region() -> RegionControl {
    let mut region = RegionControl {
        enabled: true,
        low: 90.0,
        high: 2_200.0,
        gain: -12.0,
        ..Default::default()
    };
    region.harmonic.enabled = true;
    region.harmonic.source = FundamentalSource::Fixed;
    region.harmonic.first = 1;
    region.harmonic.last = 4;
    region.harmonic.width_percent = 40.0;
    region
}

fn response(scope: Scope, source: Value, target: Value, strength: f32, reference: f32) -> Response {
    Response {
        enabled: true,
        scope,
        source,
        target,
        strength,
        reference,
        ..Default::default()
    }
}

#[test]
fn whole_response() {
    let mut region = region();
    region.resonance.enabled = true;
    region.resonance.q = 4.0;
    region.harmonic.source = FundamentalSource::Tracked;
    region.responses = [
        response(Scope::Whole, Value::Gain, Value::ResonanceQ, -1.0, -6.0),
        response(
            Scope::Whole,
            Value::Fundamental,
            Value::HarmonicWidth,
            1.0,
            110.0,
        ),
    ];
    let effective = whole(region, Some(220.0), RATE);
    assert_eq!(effective.resonance.q, 8.0);
    assert_eq!(effective.harmonic.width_percent, 80.0);
    assert_eq!(whole(region, None, RATE).harmonic.width_percent, 40.0);

    region.responses[1] = region.responses[0];
    assert_eq!(whole(region, None, RATE).resonance.q, 16.0);

    region.resonance.q = 32.0;
    region.responses[1].strength = 1.0;
    assert_eq!(whole(region, None, RATE).resonance.q, 16.0);
    region.reverse_responses = true;
    assert_eq!(whole(region, None, RATE).resonance.q, 32.0);

    region.resonance.enabled = false;
    assert_eq!(whole(region, None, RATE).resonance.q, 32.0);
}

#[test]
fn response_routes_and_curves() {
    let mut region = region();
    region.responses = [
        response(Scope::Whole, Value::Gain, Value::Center, 1.0, -18.0),
        response(
            Scope::Whole,
            Value::Center,
            Value::Gain,
            1.0,
            (90.0_f32 * 2_200.0).sqrt(),
        ),
    ];
    let out = whole(region, None, RATE);
    assert_eq!(
        out.gain, region.gain,
        "the second slot reads the pre-Response center"
    );
    assert!((out.low / region.low - 2.0).abs() < 1e-5);

    let mut mapping = response(
        Scope::Harmonic,
        Value::HarmonicFrequency,
        Value::HarmonicWidth,
        1.0,
        100.0,
    );
    for (curve, expected) in [
        (Curve::Power, 80.0),
        (Curve::Linear, 80.0),
        (Curve::Inverse, 20.0),
        (Curve::Ease, 80.0),
    ] {
        mapping.curve = curve;
        assert_eq!(mapping.output(200.0, 40.0, RATE), expected);
    }

    mapping.curve = Curve::Power;
    assert!((mapping.output(100.0 * 0.25_f32.exp2(), 40.0, RATE) - 47.568).abs() < 0.01);
    mapping.curve = Curve::Ease;
    assert!((mapping.output(100.0 * 0.25_f32.exp2(), 40.0, RATE) - 44.576).abs() < 0.01);
    mapping.strength = 0.0;
    assert_eq!(mapping.output(200.0, 40.0, RATE), 40.0);
    mapping.strength = -1.0;
    assert_eq!(mapping.output(0.0, 40.0, RATE), 40.0);
    assert_eq!(mapping.output(f32::NAN, 40.0, RATE), 40.0);
    mapping.source = mapping.target;
    assert!(!mapping.valid(region));
}

#[test]
fn harmonic_bounds() {
    let mut region = region();
    region.harmonic.fixed_hz = 2000.0;
    region.low = 10.0;
    region.high = 30_000.0;
    region.harmonic.first = 16;
    region.harmonic.last = 1;
    let harmonics = region.harmonics(48_000.0, None);
    assert_eq!(harmonics.iter().flatten().count(), 11);
    assert_eq!(harmonics[0].unwrap().number, 1);
    assert!(harmonics[15].is_none());
    let harmonics = region.harmonics(96_000.0, None);
    assert_eq!(harmonics.iter().flatten().count(), 15);
    region.harmonic.fixed_hz = 1000.0;
    region.harmonic.first = 15;
    region.harmonic.last = 16;
    assert_eq!(region.harmonics(RATE, None)[15].unwrap().number, 16);
    region.harmonic.enabled = false;
    assert!(region.harmonics(RATE, None).iter().all(Option::is_none));
}

#[test]
fn harmonic_response_order() {
    let mut region = region();
    region.harmonic.fixed_hz = 110.0;
    region.harmonic.width_percent = 70.0;
    let expand = response(
        Scope::Harmonic,
        Value::HarmonicFrequency,
        Value::HarmonicWidth,
        1.0,
        220.0,
    );
    region.responses = [
        expand,
        Response {
            strength: -1.0,
            ..expand
        },
    ];

    assert_eq!(region.harmonics(RATE, None)[3].unwrap().width, 50.0);
    region.reverse_responses = true;
    assert_eq!(region.harmonics(RATE, None)[3].unwrap().width, 70.0);
}

fn signal(frames: usize, rate: f32, frequencies: &[f32]) -> Vec<f32> {
    (0..frames)
        .map(|i| {
            frequencies
                .iter()
                .map(|hz| (std::f32::consts::TAU * *hz * i as f32 / rate).sin() * 0.1)
                .sum()
        })
        .collect()
}

fn render(region: RegionControl, size: usize, rate: f32, input: &[f32]) -> Vec<f32> {
    let mut engine = Engine::new(rate);
    let mut controls = controls(flat_bands());
    controls.regions[0] = region;
    let mut left = input.to_vec();
    let mut right = input.to_vec();
    nice_assert_no_alloc::assert_no_alloc(|| {
        for (left, right) in left.chunks_mut(size).zip(right.chunks_mut(size)) {
            engine.process_block(left, right, &controls);
        }
    });
    assert_eq!(engine.latency(), 0);
    assert!(left.iter().all(|sample| sample.is_finite()));
    left
}

fn amplitude(samples: &[f32], hz: f32) -> f64 {
    let samples = &samples[samples.len() - RATE as usize..];
    let (a, b) = samples
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(a, b), (i, v)| {
            let phase = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(RATE);
            (
                a + f64::from(*v) * phase.cos(),
                b + f64::from(*v) * phase.sin(),
            )
        });
    a.hypot(b) * 2.0 / samples.len() as f64
}

#[test]
fn per_harmonic_audio() {
    let base = region();
    let mut changed = base;
    changed.responses[0] = response(
        Scope::Harmonic,
        Value::HarmonicFrequency,
        Value::HarmonicWidth,
        -1.0,
        110.0,
    );
    let plan = changed.harmonics(RATE, None);
    assert_eq!(plan[0].unwrap().width, 40.0);
    assert_eq!(plan[3].unwrap().width, 10.0);
    changed.harmonic.fixed_hz = 220.0;
    assert_eq!(changed.harmonics(RATE, None)[3].unwrap().width, 5.0);
    changed.harmonic.fixed_hz = 110.0;

    let input = signal(96_000, RATE, &[121.0, 440.0, 451.0]);
    let dry = render(base, 128, RATE, &input);
    let narrowed = render(changed, 128, RATE, &input);
    assert!(amplitude(&narrowed, 451.0) > 1.3 * amplitude(&dry, 451.0));
    assert!((amplitude(&dry, 121.0) - amplitude(&narrowed, 121.0)).abs() < 0.002);

    changed.responses[1] = response(
        Scope::Harmonic,
        Value::HarmonicIndex,
        Value::HarmonicDepth,
        -1.0,
        1.0,
    );
    assert_eq!(changed.harmonics(RATE, None)[3].unwrap().depth, 0.0);
    let reduced = render(changed, 128, RATE, &input);
    assert!(amplitude(&reduced, 440.0) > 1.5 * amplitude(&narrowed, 440.0));

    for slot in &mut changed.responses {
        slot.bypass = true;
    }

    assert_eq!(render(changed, 128, RATE, &input), dry);
}

#[test]
fn response_block_size_and_pitch() {
    let mut region = region();
    region.harmonic.source = FundamentalSource::Tracked;
    region.harmonic.detection.algorithm = crate::pitch::DetectionMode::Mpm;
    region.responses[0] = response(
        Scope::Whole,
        Value::Fundamental,
        Value::HarmonicWidth,
        1.0,
        110.0,
    );

    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let input = signal(rate as usize / 2, rate, &[110.0, 220.0, 440.0]);
        let expected = render(region, 32, rate, &input);
        for size in [64, 128, 1024] {
            assert_eq!(render(region, size, rate, &input), expected);
        }
    }

    let mut engine = Engine::new(RATE);
    let mut controls = controls(flat_bands());
    controls.regions[0] = region;
    let mut left = signal(48_000, RATE, &[110.0]);
    let mut right = left.clone();
    engine.process_block(&mut left, &mut right, &controls);
    assert!((engine.harmonic_hz(0).unwrap() - 110.0).abs() < 2.0);
    left.fill(0.0);
    right.fill(0.0);
    engine.process_block(&mut left, &mut right, &controls);
    assert_eq!(engine.harmonic_hz(0), None);
}

#[test]
fn response_automation_continuity() {
    let mut region = region();
    region.responses[0] = response(
        Scope::Harmonic,
        Value::HarmonicFrequency,
        Value::HarmonicWidth,
        1.0,
        110.0,
    );
    let mut engine = Engine::new(RATE);
    let mut controls = controls(flat_bands());
    controls.regions[0] = region;

    let mut left = signal(48_000, RATE, &[451.0]);
    let mut right = left.clone();

    nice_assert_no_alloc::assert_no_alloc(|| {
        for (i, (left, right)) in left.chunks_mut(128).zip(right.chunks_mut(128)).enumerate() {
            let slot = &mut controls.regions[0].responses[0];
            slot.strength = if i % 16 < 8 { 1.0 } else { -1.0 };
            slot.bypass = i % 23 < 3;
            engine.process_block(left, right, &controls);
        }
    });

    let max_step = left
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    assert!(max_step < 0.015, "maximum sample step {max_step:.6}");
}
