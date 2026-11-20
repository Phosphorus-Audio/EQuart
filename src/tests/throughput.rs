use super::*;

// throughput

fn bench_case(label: &str, controls: Controls, frames: usize, analyze: bool) -> f64 {
    use std::time::Instant;

    const BLOCK: usize = 256;

    let mut engine = Engine::new(RATE);
    engine.analyze = analyze;
    let mut left = vec![0.0_f32; BLOCK];
    let mut right = vec![0.0_f32; BLOCK];

    let mut state = 0x1234_5678u32;
    let mut fill = |left: &mut [f32], right: &mut [f32]| {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *l = ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.1;
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *r = ((state >> 8) as f32 / 8_388_608.0 - 1.0) * 0.1;
        }
    };

    for _ in 0..16 {
        fill(&mut left, &mut right);
        engine.process_block(&mut left, &mut right, &controls);
    }

    // take best of a few runs to avoid noisy shared machine spikes
    let mut best = f64::INFINITY;
    for _ in 0..5 {
        let start = Instant::now();
        let mut done = 0;
        while done < frames {
            fill(&mut left, &mut right);
            engine.process_block(&mut left, &mut right, &controls);
            done += BLOCK;
        }
        best = best.min(start.elapsed().as_nanos() as f64 / done as f64);
    }

    println!("{label:<26} {best:8.2} ns/frame");
    best
}

#[test]
#[ignore = "manual throughput benchmark"]
fn bench_engine_throughput() {
    let frames = RATE as usize;

    let bell = |i: usize| BandControl {
        enabled: true,
        frequency: 60.0 * 1.6_f32.powi(i as i32),
        gain: 6.0,
        q: 1.4,
        kind: FilterKind::Peak,
        slope: CutSlope::default(),
        channel: ChannelMode::Stereo,
        dynamics: DynamicControl::default(),
        pitch: Default::default(),
    };

    let cut = |i: usize| BandControl {
        enabled: true,
        frequency: 80.0 * 1.55_f32.powi(i as i32),
        gain: 0.0,
        q: 0.7071,
        kind: FilterKind::Lowpass,
        slope: CutSlope::Db96,
        channel: ChannelMode::Stereo,
        dynamics: DynamicControl::default(),
        pitch: Default::default(),
    };

    let idle = |_: usize| BandControl {
        enabled: false,
        ..BandControl::default()
    };

    let make = |f: &dyn Fn(usize) -> BandControl| Controls {
        bands: std::array::from_fn(f),
        regions: Default::default(),
        output_db: 0.0,
        mix: 1.0,
        bypass: false,
        oversampling: crate::oversample::Oversampling::Off,
        precision: Default::default(),
    };

    println!();
    bench_case("all bands off", make(&idle), frames, false);
    bench_case("8 bells", make(&bell), frames, false);
    let worst = bench_case("8 cuts @ 96 dB/oct", make(&cut), frames, false);
    bench_case("8 cuts, analyser on", make(&cut), frames, true);

    let with_os = |bands: [BandControl; crate::band::BAND_COUNT],
                   oversampling: crate::oversample::Oversampling| Controls {
        bands,
        regions: Default::default(),
        output_db: 0.0,
        mix: 1.0,
        bypass: false,
        oversampling,
        precision: Default::default(),
    };
    bench_case(
        "8 cuts, 2x",
        with_os(
            std::array::from_fn(&cut),
            crate::oversample::Oversampling::X2,
        ),
        frames,
        false,
    );
    bench_case(
        "8 cuts, 4x",
        with_os(
            std::array::from_fn(&cut),
            crate::oversample::Oversampling::X4,
        ),
        frames,
        false,
    );

    println!(
        "
worst case: {:.2} ns/frame, {:.3}% of one 48 kHz core (stereo)",
        worst,
        worst * RATE as f64 / 1e9 * 100.0
    );

    // only check release build, debug is way slower
    let limit = if cfg!(debug_assertions) {
        6_000.0
    } else {
        300.0
    };
    assert!(
        worst < limit,
        "engine is unexpectedly slow: {worst} ns/frame"
    );
}
