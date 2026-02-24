use anyhow::Context;
use tracing_subscriber::EnvFilter;

pub fn init(verbose: bool) -> anyhow::Result<()> {
    let default_level = if verbose { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_level))
        .with_context(|| format!("invalid log filter level: {default_level}"))?;

    let formatter = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact();

    if verbose {
        formatter.init();
    } else {
        formatter.without_time().init();
    }

    Ok(())
}
