/// guard against stack overflow on plugin init (used to be 219 KiB and crash DAWs)
#[test]
fn init_fits_host_stack() {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let plugin = crate::Equart::default();
            assert_eq!(plugin.params.bands.len(), crate::band::BAND_COUNT);
        })
        .expect("spawn")
        .join()
        .expect("the plug-in must build on a 512 KiB stack");
}

#[test]
fn default_patch_preserves_host_samples() {
    use crate::precision::Precision;

    let mut plugin = crate::Equart::default();
    let mut controls = plugin.controls();

    assert!(!controls.bypass);

    let amplitude = 10.0_f64.powf(-6.34 / 20.0);

    let period: Vec<_> = (0..48_000)
        .map(|frame| {
            let phase = std::f64::consts::TAU * f64::from((999 * frame) % 48_000) / 48_000.0;
            [
                (amplitude * phase.sin()) as f32,
                (amplitude * phase.cos()) as f32,
            ]
        })
        .collect();

    for precision in [Precision::Single, Precision::Double] {
        controls.precision = precision;


        for input in period.chunks(257) {
            let mut left: Vec<_> = input.iter().map(|frame| frame[0]).collect();
            let mut right: Vec<_> = input.iter().map(|frame| frame[1]).collect();
            plugin
                .engine
                .process_block(&mut left, &mut right, &controls);
            for ((left, right), original) in left.into_iter().zip(right).zip(input) {
                assert_eq!(
                    [left.to_bits(), right.to_bits()],
                    original.map(f32::to_bits)
                );
            }
        }

        assert_eq!(plugin.engine.latency(), 0);
    }
}
