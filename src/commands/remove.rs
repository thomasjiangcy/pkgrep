use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use tracing::warn;

use crate::config::Config;
use crate::depspec;
use crate::index;
use crate::source;

pub(super) fn run_remove(
    cwd: &Path,
    config: &Config,
    dep_specs: Vec<String>,
    yes: bool,
) -> anyhow::Result<()> {
    let parsed_specs = super::parse_dep_specs(&dep_specs)?;

    if !yes {
        warn!(
            dep_spec_count = parsed_specs.len(),
            "remove called without --yes; no-op"
        );
        println!(
            "No-op: pass --yes to remove linked dependencies under {}/.pkgrep/deps",
            cwd.display()
        );
        return Ok(());
    }

    tracing::info!(dep_spec_count = parsed_specs.len(), "remove requested");
    let cache_root = source::cache_root_for(cwd, &config.cache_dir);

    let mut removed = 0usize;
    let mut not_found = 0usize;
    let mut skipped = 0usize;

    for spec in parsed_specs {
        let candidate_paths = if let Some(version) = spec.version {
            vec![cwd.join(depspec::link_path(&spec.ecosystem, &spec.locator, &version))]
        } else {
            let locator_prefix_path =
                cwd.join(depspec::link_path_prefix(&spec.ecosystem, &spec.locator));
            let Some(parent_dir) = locator_prefix_path.parent() else {
                not_found += 1;
                continue;
            };
            let Some(file_name) = locator_prefix_path.file_name() else {
                not_found += 1;
                continue;
            };
            let locator_prefix = file_name.to_string_lossy().to_string();
            discover_matching_links(parent_dir, &locator_prefix)?
        };

        if candidate_paths.is_empty() {
            not_found += 1;
            continue;
        }

        for candidate in candidate_paths {
            match remove_link_candidate(&candidate)? {
                RemoveOutcome::Removed { symlink_target } => {
                    removed += 1;
                    if let Err(err) = index::record_unlink(
                        cwd,
                        &cache_root,
                        &candidate,
                        symlink_target.as_deref(),
                    ) {
                        warn!(candidate = %candidate.display(), error = %err, "failed to update local index files after remove");
                    }
                }
                RemoveOutcome::NotFound => not_found += 1,
                RemoveOutcome::Skipped => skipped += 1,
            }
        }
    }

    println!(
        "Remove completed: removed={} not_found={} skipped={} (non-symlink paths are skipped)",
        removed, not_found, skipped
    );
    Ok(())
}

fn discover_matching_links(links_dir: &Path, locator_prefix: &str) -> anyhow::Result<Vec<PathBuf>> {
    if !links_dir.exists() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    for entry in fs::read_dir(links_dir)
        .with_context(|| format!("failed to read link directory {}", links_dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!(
                "failed to read entry from link directory {}",
                links_dir.display()
            )
        })?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with(locator_prefix) {
            matches.push(entry.path());
        }
    }

    Ok(matches)
}

enum RemoveOutcome {
    Removed { symlink_target: Option<PathBuf> },
    NotFound,
    Skipped,
}

fn remove_link_candidate(candidate: &Path) -> anyhow::Result<RemoveOutcome> {
    let metadata = match fs::symlink_metadata(candidate) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RemoveOutcome::NotFound);
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to inspect candidate path for removal {}",
                    candidate.display()
                )
            });
        }
    };

    if metadata.file_type().is_symlink() {
        let symlink_target = fs::read_link(candidate).ok();
        fs::remove_file(candidate)
            .with_context(|| format!("failed to remove candidate path {}", candidate.display()))?;
        return Ok(RemoveOutcome::Removed { symlink_target });
    }

    if metadata.is_file() {
        fs::remove_file(candidate)
            .with_context(|| format!("failed to remove candidate path {}", candidate.display()))?;
        return Ok(RemoveOutcome::Removed {
            symlink_target: None,
        });
    }

    warn!(
        candidate = %candidate.display(),
        "skipping non-symlink directory while removing links"
    );
    Ok(RemoveOutcome::Skipped)
}
