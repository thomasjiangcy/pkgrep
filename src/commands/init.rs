use std::fs;
use std::path::Path;

use anyhow::Context;

use crate::commands::skill;

const GITIGNORE_ENTRY: &str = ".pkgrep/";
const AGENTS_SECTION_START: &str = "<!-- pkgrep:init:start -->";
const AGENTS_SECTION_END: &str = "<!-- pkgrep:init:end -->";

pub(super) fn run_init(cwd: &Path) -> anyhow::Result<()> {
    let gitignore_updated = ensure_gitignore(cwd)?;
    let agents_updated = ensure_agents_md(cwd)?;
    let skill_installed = skill::install_project_skill_if_missing(cwd)?;

    if gitignore_updated {
        println!("Updated .gitignore");
    } else {
        println!(".gitignore already configured");
    }

    if agents_updated {
        println!("Updated AGENTS.md");
    } else {
        println!("AGENTS.md already configured");
    }

    if skill_installed {
        println!("Installed project skill");
    } else {
        println!("Project skill already installed");
    }

    Ok(())
}

fn ensure_gitignore(cwd: &Path) -> anyhow::Result<bool> {
    let path = cwd.join(".gitignore");
    let existing = if path.exists() {
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };

    if existing.lines().any(|line| line.trim() == GITIGNORE_ENTRY) {
        return Ok(false);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GITIGNORE_ENTRY);
    updated.push('\n');

    fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn ensure_agents_md(cwd: &Path) -> anyhow::Result<bool> {
    let path = cwd.join("AGENTS.md");
    let section = agents_section();
    let existing = if path.exists() {
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::from(
            "# AGENTS.md\n\nInstructions for AI coding agents working with this codebase.\n",
        )
    };

    if existing.contains(AGENTS_SECTION_START) {
        let start = existing.find(AGENTS_SECTION_START).ok_or_else(|| {
            anyhow::anyhow!("failed to locate existing pkgrep AGENTS section start")
        })?;
        let end = existing.find(AGENTS_SECTION_END).ok_or_else(|| {
            anyhow::anyhow!("failed to locate existing pkgrep AGENTS section end")
        })?;
        let suffix_start = end + AGENTS_SECTION_END.len();
        let current = &existing[start..suffix_start];
        if current == section {
            return Ok(false);
        }

        let mut updated = String::new();
        updated.push_str(&existing[..start]);
        updated.push_str(section);
        updated.push_str(&existing[suffix_start..]);
        fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))?;
        return Ok(true);
    }

    let mut updated = existing;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.ends_with("\n\n") {
        updated.push('\n');
    }
    updated.push_str(section);
    updated.push('\n');

    fs::write(&path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(true)
}

fn agents_section() -> &'static str {
    concat!(
        "<!-- pkgrep:init:start -->\n",
        "## pkgrep\n\n",
        "Dependency source links are stored under `.pkgrep/deps/`.\n\n",
        "Use `pkgrep pull <dep-spec>` to link dependency source into this project.\n",
        "Use `pkgrep list` to inspect which dependency sources are currently linked.\n",
        "When reporting findings from pulled dependency code, quote or summarize the relevant code inline instead of only referencing local `.pkgrep` paths.\n",
        "Use the bundled `pkgrep-usage` skill from `.agents/skills/pkgrep-usage/` when traversing dependency source.\n",
        "<!-- pkgrep:init:end -->"
    )
}
