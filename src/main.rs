mod cli;
mod commands;
mod config;
mod depspec;
mod error;
mod index;
mod installed_version;
mod logging;
mod providers;
mod registry_resolver;
mod source;

use anyhow::Context;
use clap::Parser;
use tracing::{error, info};

use crate::cli::{CacheCommand, Cli, Command, SelfCommand, SkillCommand};
use crate::config::Config;

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
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Pull { .. } => "pull",
        Command::Remove { .. } => "remove",
        Command::Path { .. } => "path",
        Command::List { .. } => "list",
        Command::Init => "init",
        Command::Cache { command } => match command {
            CacheCommand::Clean { .. } => "cache_clean",
            CacheCommand::Prune { .. } => "cache_prune",
        },
        Command::Skill { command } => match command {
            SkillCommand::Install { .. } => "skill_install",
        },
        Command::SelfCmd { command } => match command {
            SelfCommand::Update => "self_update",
        },
    }
}
