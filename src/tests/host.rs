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
