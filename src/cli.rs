use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "pkgrep",
    version,
    about = "Dependency source cache helper for developers and coding agents"
)]
pub struct Cli {
    /// Enable verbose logging (debug level).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Pull dependency source code into cache and link into the project.
    Pull {
        /// Dependency spec(s), for example:
        /// git:https://github.com/org/repo.git@<rev>
        /// git:https://github.com/org/repo.git#<rev> (useful when rev contains '@')
        /// npm:zod@<version>
        /// pypi:requests@<version>
        /// zod@<version> (implicit npm/pypi when a single supported project lockfile ecosystem is detected)
        dep_specs: Vec<String>,
    },

    /// Remove linked dependency sources from .pkgrep/deps.
    Remove {
        /// Dependency spec(s) to remove.
        #[arg(required = true)]
        dep_specs: Vec<String>,

        /// Required for destructive action.
        #[arg(long)]
        yes: bool,
    },

    /// Resolve linked path for a dependency in the current project.
    Path {
        /// Dependency spec to resolve.
        dep_spec: String,
    },

    /// Cache operations.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },

    /// Skill operations.
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },

    /// Self-management operations.
    #[command(name = "self")]
    SelfCmd {
        #[command(subcommand)]
        command: SelfCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Hydrate local cache entries from remote object store cache.
    Hydrate {
        /// Optional dependency spec(s). If omitted, use project files from cwd.
        dep_specs: Vec<String>,
    },

    /// Clean local cache entries.
    Clean {
        /// Required for destructive action.
        #[arg(long)]
        yes: bool,
    },

    /// Prune unreferenced cached checkouts and mirrors.
    Prune {
        /// Required for destructive action.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Clone, Debug, ValueEnum)]
pub enum SkillInstallMode {
    Project,
    Global,
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Install the bundled pkgrep usage skill.
    Install {
        /// Install target mode. Defaults to project.
        #[arg(long, value_enum, default_value_t = SkillInstallMode::Project)]
        mode: SkillInstallMode,

        /// Explicit skills directory. Overrides mode-based default target root.
        #[arg(long)]
        target: Option<PathBuf>,

        /// Replace an existing installed skill directory.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SelfCommand {
    /// Update pkgrep to the latest GitHub Release for this platform.
    Update,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cache_clean_yes() {
        let cli = Cli::try_parse_from(["pkgrep", "cache", "clean", "--yes"]).expect("parse");
        match cli.command {
            Command::Cache {
                command: CacheCommand::Clean { yes },
            } => assert!(yes),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_cache_prune_yes() {
        let cli = Cli::try_parse_from(["pkgrep", "cache", "prune", "--yes"]).expect("parse");
        match cli.command {
            Command::Cache {
                command: CacheCommand::Prune { yes },
            } => assert!(yes),
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn remove_requires_dep_spec() {
        let result = Cli::try_parse_from(["pkgrep", "remove", "--yes"]);
        assert!(result.is_err());
    }

    #[test]
    fn parses_verbose_flag() {
        let cli = Cli::try_parse_from(["pkgrep", "--verbose", "pull"]).expect("parse");
        assert!(cli.verbose);
    }

    #[test]
    fn parses_path_command() {
        let cli = Cli::try_parse_from(["pkgrep", "path", "git:https://example.com/repo.git@v1"])
            .expect("parse");
        match cli.command {
            Command::Path { dep_spec } => {
                assert_eq!(dep_spec, "git:https://example.com/repo.git@v1")
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_skill_install_global_force_with_target() {
        let cli = Cli::try_parse_from([
            "pkgrep",
            "skill",
            "install",
            "--mode",
            "global",
            "--target",
            "/tmp/skills",
            "--force",
        ])
        .expect("parse");
        match cli.command {
            Command::Skill {
                command:
                    SkillCommand::Install {
                        mode,
                        target,
                        force,
                    },
            } => {
                assert!(matches!(mode, SkillInstallMode::Global));
                assert_eq!(target.as_deref(), Some(std::path::Path::new("/tmp/skills")));
                assert!(force);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn parses_self_update_command() {
        let cli = Cli::try_parse_from(["pkgrep", "self", "update"]).expect("parse");
        match cli.command {
            Command::SelfCmd {
                command: SelfCommand::Update,
            } => {}
            _ => panic!("unexpected command"),
        }
    }
}
