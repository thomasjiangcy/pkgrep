mod cache;
mod path;
mod pull;
mod remove;
mod self_update;
mod skill;

use std::path::Path;

use crate::cli::{CacheCommand, Command, SelfCommand, SkillCommand};
use crate::config::Config;
use crate::depspec::DepSpec;

pub fn execute(cwd: &Path, config: &Config, command: Command) -> anyhow::Result<()> {
    match command {
        Command::Pull { dep_specs } => pull::run_pull(cwd, config, dep_specs),
        Command::Remove { dep_specs, yes } => remove::run_remove(cwd, config, dep_specs, yes),
        Command::Path { dep_spec } => path::run_path(cwd, dep_spec),
        Command::Cache { command } => match command {
            CacheCommand::Hydrate { dep_specs } => cache::run_cache_hydrate(cwd, config, dep_specs),
            CacheCommand::Clean { yes } => cache::run_cache_clean(cwd, config, yes),
            CacheCommand::Prune { yes } => cache::run_cache_prune(cwd, config, yes),
        },
        Command::Skill { command } => match command {
            SkillCommand::Install {
                mode,
                target,
                force,
            } => skill::run_skill_install(cwd, mode, target, force),
        },
        Command::SelfCmd { command } => match command {
            SelfCommand::Update => self_update::run_self_update(),
        },
    }
}

fn parse_dep_specs(dep_specs: &[String]) -> anyhow::Result<Vec<DepSpec>> {
    dep_specs
        .iter()
        .map(|spec| {
            crate::depspec::parse(spec)
                .map_err(|err| anyhow::anyhow!("invalid dep spec '{}': {err}", spec))
        })
        .collect()
}
