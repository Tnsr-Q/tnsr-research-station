pub fn init() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_can_be_called_multiple_times() {
        // First init should succeed silently
        init();
        // Second init should also succeed (try_init returns error but we ignore it)
        init();
    }

    #[test]
    fn test_init_is_safe() {
        // Should not panic
        init();
    }
}
