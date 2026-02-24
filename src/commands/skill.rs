use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::cli::SkillInstallMode;

const SKILL_NAME: &str = "pkgrep-usage";

struct EmbeddedSkillFile {
    relative_path: &'static str,
    contents: &'static [u8],
}

const EMBEDDED_SKILL_FILES: &[EmbeddedSkillFile] = &[
    EmbeddedSkillFile {
        relative_path: "SKILL.md",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/skills/pkgrep-usage/SKILL.md"
        )),
    },
    EmbeddedSkillFile {
        relative_path: "agents/openai.yaml",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/skills/pkgrep-usage/agents/openai.yaml"
        )),
    },
    EmbeddedSkillFile {
        relative_path: "references/commands.md",
        contents: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/skills/pkgrep-usage/references/commands.md"
        )),
    },
];

pub(super) fn run_skill_install(
    cwd: &Path,
    mode: SkillInstallMode,
    target: Option<PathBuf>,
    force: bool,
) -> anyhow::Result<()> {
    let target_root = match target {
        Some(path) => path,
        None => default_target_root(cwd, mode)?,
    };

    fs::create_dir_all(&target_root).with_context(|| {
        format!(
            "failed to create skills directory {}",
            target_root.display()
        )
    })?;

    let destination = target_root.join(SKILL_NAME);

    if destination.exists() {
        if force {
            fs::remove_dir_all(&destination).with_context(|| {
                format!(
                    "failed to remove existing installed skill at {}",
                    destination.display()
                )
            })?;
        } else {
            anyhow::bail!(
                "skill destination already exists: {} (rerun with --force to replace)",
                destination.display()
            );
        }
    }

    install_embedded_skill(&destination)?;

    println!("Installed skill: {}", destination.display());
    println!("Restart your agent runtime to load new skills");

    Ok(())
}

fn default_target_root(cwd: &Path, mode: SkillInstallMode) -> anyhow::Result<PathBuf> {
    match mode {
        SkillInstallMode::Project => Ok(cwd.join(".agents").join("skills")),
        SkillInstallMode::Global => {
            let home = dirs::home_dir().ok_or_else(|| {
                anyhow::anyhow!("unable to resolve home directory for --mode global")
            })?;
            Ok(home.join(".agents").join("skills"))
        }
    }
}

fn install_embedded_skill(destination: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(destination).with_context(|| {
        format!(
            "failed to create skill destination {}",
            destination.display()
        )
    })?;

    for file in EMBEDDED_SKILL_FILES {
        let path = destination.join(file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create skill subdirectory {}", parent.display())
            })?;
        }

        fs::write(&path, file.contents)
            .with_context(|| format!("failed to write embedded skill file {}", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::cli::SkillInstallMode;
    use crate::commands::skill::default_target_root;

    #[test]
    fn default_project_target_is_agents_skills_under_cwd() {
        let cwd = Path::new("/tmp/project");
        let target = default_target_root(cwd, SkillInstallMode::Project).expect("target");
        assert_eq!(target, PathBuf::from("/tmp/project/.agents/skills"));
    }
}
