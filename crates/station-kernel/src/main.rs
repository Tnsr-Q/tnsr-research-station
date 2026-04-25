fn main() {
    station_telemetry::init();

    if let Err(err) = station_kernel::run_from_env() {
        eprintln!("station kernel failed: {err}");
        std::process::exit(1);
    }
}
