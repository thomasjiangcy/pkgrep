mod cli;
mod commands;
mod config;
mod depspec;
mod error;
mod index;
mod logging;
mod providers;
mod registry_resolver;
mod remote_cache;
mod source;

use anyhow::Context;
use clap::Parser;
use tracing::{debug, error, info};

use crate::cli::{CacheCommand, Cli, Command};
use crate::config::{Config, ObjectStoreAuthMode};

fn main() {
    if let Err(err) = run() {
        error!(error = %err, "command failed");
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    logging::init(cli.verbose)?;

    let cwd = std::env::current_dir().context("failed to get current working directory")?;
    let config = config::load(&cwd).context("failed to load configuration")?;

    log_command_start(&cwd, &config, &cli.command);

    commands::execute(&cwd, &config, cli.command)
}

fn log_command_start(cwd: &std::path::Path, config: &Config, command: &Command) {
    info!(
        command = command_name(command),
        cwd = %cwd.display(),
        backend = %config.backend,
        worker_pool_size = config.worker_pool_size,
        "starting command"
    );

    debug!(
        object_store_bucket = config.object_store.bucket.as_deref().unwrap_or("<unset>"),
        object_store_prefix = config.object_store.prefix.as_deref().unwrap_or("<unset>"),
        object_store_endpoint = config.object_store.endpoint.as_deref().unwrap_or("<unset>"),
        object_store_auth_mode = object_store_auth_mode(config),
        object_store_proxy_identity_header = config
            .object_store
            .proxy_identity_header
            .as_deref()
            .unwrap_or("<unset>"),
        "resolved object store settings"
    );
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Pull { .. } => "pull",
        Command::Remove { .. } => "remove",
        Command::Path { .. } => "path",
        Command::Cache { command } => match command {
            CacheCommand::Hydrate { .. } => "cache_hydrate",
            CacheCommand::Clean { .. } => "cache_clean",
            CacheCommand::Prune { .. } => "cache_prune",
        },
    }
}

fn object_store_auth_mode(config: &Config) -> &'static str {
    match config.object_store.auth_mode {
        Some(ObjectStoreAuthMode::Direct) => "direct",
        Some(ObjectStoreAuthMode::Proxy) => "proxy",
        None => "<unset>",
    }
}
