//! Subscriber filters for executable entry points.
use tracing::{Level, Subscriber};
use tracing_subscriber::{filter::filter_fn, registry::LookupSpan, EnvFilter, Layer};

/// Parse user directives, falling back to the executable's default. A separate
/// metadata gate caps every dependency at info, even inside user-enabled spans
/// or with more specific target directives.
pub fn filter<S>(default: &str, directives: Option<&str>) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let requested = directives
        .and_then(|s| EnvFilter::try_new(s).ok())
        .unwrap_or_else(|| EnvFilter::new(default));
    requested.and_then(filter_fn(|metadata| {
        *metadata.level() <= Level::INFO
            || ["flek", "flek_core", "flek_cli", "flek_app"]
                .iter()
                .any(|target| {
                    metadata.target() == *target
                        || metadata
                            .target()
                            .strip_prefix(target)
                            .is_some_and(|s| s.starts_with("::"))
                })
    }))
}

/// CLI verbosity applies to Flek targets.
pub fn cli_default(verbose: u8) -> &'static str {
    match verbose {
        0 => "warn",
        1 => "warn,flek=info,flek_core=info,flek_cli=info,flek_app=info",
        2 => "warn,flek=debug,flek_core=debug,flek_cli=debug,flek_app=debug",
        _ => "warn,flek=trace,flek_core=trace,flek_cli=trace,flek_app=trace",
    }
}
