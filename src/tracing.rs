//! Tracing initialization helpers for the notes service.

use ::tracing::info;
use std::env;

/// Initializes tracing using the workspace logging conventions.
pub(crate) fn init_tracing(service_name: &'static str) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let log_format = env::var("ELOWEN_LOG_FORMAT").unwrap_or_else(|_| "plain".to_string());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true);

    if log_format.eq_ignore_ascii_case("json") {
        builder
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .flatten_event(true)
            .with_ansi(false)
            .init();
    } else {
        builder.with_ansi(true).init();
    }

    info!(service = service_name, log_format = %log_format, "tracing initialized");
}
