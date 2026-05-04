use std::path::Path;

use anyhow::Result;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init(verbose: u8, log_file: Option<&Path>) -> Result<()> {
    let default_filter = match verbose {
        0 => "info,btleplug=warn",
        1 => "debug,btleplug=info",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter));

    let stdout_layer = fmt::layer()
        .with_target(false)
        .with_timer(fmt::time::ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f %:z".into()));

    let registry = tracing_subscriber::registry().with(filter).with(stdout_layer);

    if let Some(path) = log_file {
        let file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        let file_layer = fmt::layer()
            .with_target(false)
            .with_writer(std::sync::Mutex::new(file))
            .with_timer(fmt::time::ChronoLocal::new(
                "%Y-%m-%d %H:%M:%S%.3f %:z".into(),
            ));
        registry.with(file_layer).init();
    } else {
        registry.init();
    }
    Ok(())
}
