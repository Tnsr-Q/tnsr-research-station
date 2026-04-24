pub fn init() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .try_init();
}
