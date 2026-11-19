fn main() {
    let success = nice_plug::nice_export_standalone::<equart::Equart>();
    std::process::exit(if success { 0 } else { 1 });
}
