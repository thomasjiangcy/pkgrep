use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use tracing::{info, warn};

use crate::config::{Backend, Config};
use crate::depspec;
use crate::index;
use crate::remote_cache;
use crate::source;

use super::pull;

pub(super) fn run_cache_hydrate(
    cwd: &Path,
    config: &Config,
    dep_specs: Vec<String>,
) -> anyhow::Result<()> {
    match config.backend {
        Backend::S3 | Backend::AzureBlob => {
            let resolved = pull::resolve_pull_resolution(cwd, &dep_specs)?;

            if dep_specs.is_empty() {
                if resolved.discovered_lockfiles == 0 {
                    println!(
                        "No-op: no dep specs provided and no supported project lockfiles found in {}",
                        cwd.display()
                    );
                    return Ok(());
                }

                if resolved.targets.is_empty() {
                    println!(
                        "No-op: detected {} dependency entries from {} lockfile(s), but none had git source hints (git-only mode).",
                        resolved.discovered_dependencies, resolved.discovered_lockfiles
                    );
                    return Ok(());
                }
            }

            info!(
                dep_spec_count = dep_specs.len(),
                hydrate_target_count = resolved.targets.len(),
                "cache hydrate requested"
            );

            let client = remote_cache::RemoteCacheClient::from_config(config)?
                .ok_or_else(|| anyhow::anyhow!("remote backend client initialization failed"))?;
            let cache_root = source::cache_root_for(cwd, &config.cache_dir);

            let mut hydrated_count = 0usize;
            let mut already_present_count = 0usize;
            let mut not_found_count = 0usize;
            let total_targets = resolved.targets.len();
            for (index, target) in resolved.targets.iter().enumerate() {
                println!(
                    "[{}/{}] hydrate {}@{}",
                    index + 1,
                    total_targets,
                    target.git_url,
                    target.requested_revision
                );

                match client
                    .hydrate_git_source(cwd, config, target)
                    .with_context(|| {
                        format!(
                            "failed to hydrate git source {}@{}",
                            target.git_url, target.requested_revision
                        )
                    })? {
                    remote_cache::HydrateOutcome::Hydrated(materialized) => {
                        hydrated_count += 1;
                        if let Err(err) =
                            index::record_link(cwd, &cache_root, target, &materialized)
                        {
                            warn!(
                                git_url = %target.git_url,
                                requested_revision = %target.requested_revision,
                                error = %err,
                                "failed to update local index files after hydrate"
                            );
                        }
                        println!(
                            "  -> hydrated and linked {}",
                            materialized.project_link_path.display()
                        );
                        info!(
                            git_url = %target.git_url,
                            requested_revision = %target.requested_revision,
                            source_fingerprint = %materialized.source_fingerprint,
                            checkout_path = %materialized.checkout_path.display(),
                            link_path = %materialized.project_link_path.display(),
                            "hydrated dependency source from remote cache"
                        );
                    }
                    remote_cache::HydrateOutcome::AlreadyPresent(materialized) => {
                        already_present_count += 1;
                        if let Err(err) =
                            index::record_link(cwd, &cache_root, target, &materialized)
                        {
                            warn!(
                                git_url = %target.git_url,
                                requested_revision = %target.requested_revision,
                                error = %err,
                                "failed to update local index files after hydrate"
                            );
                        }
                        println!(
                            "  -> already present locally; refreshed link {}",
                            materialized.project_link_path.display()
                        );
                        info!(
                            git_url = %target.git_url,
                            requested_revision = %target.requested_revision,
                            source_fingerprint = %materialized.source_fingerprint,
                            checkout_path = %materialized.checkout_path.display(),
                            link_path = %materialized.project_link_path.display(),
                            "dependency source already present locally; refreshed project link"
                        );
                    }
                    remote_cache::HydrateOutcome::NotFound => {
                        not_found_count += 1;
                        println!("  -> not found in remote cache");
                        warn!(
                            git_url = %target.git_url,
                            requested_revision = %target.requested_revision,
                            "dependency source not found in remote cache"
                        );
                    }
                }
            }

            println!(
                "Hydrate completed: total={} hydrated={} already_present={} not_found={}",
                resolved.targets.len(),
                hydrated_count,
                already_present_count,
                not_found_count
            );
            Ok(())
        }
        Backend::Local | Backend::AgentFs => {
            anyhow::bail!(
                "hydrate_requires_remote_backend: cache hydrate requires backend=s3 or backend=azure_blob"
            )
        }
    }
}

pub(super) fn run_cache_clean(cwd: &Path, config: &Config, yes: bool) -> anyhow::Result<()> {
    let cache_dir = if config.cache_dir.is_absolute() {
        config.cache_dir.clone()
    } else {
        cwd.join(&config.cache_dir)
    };

    if !yes {
        warn!(cache_dir = %cache_dir.display(), "cache clean called without --yes; no-op");
        println!(
            "No-op: pass --yes to clean local cache at {}",
            cache_dir.display()
        );
        return Ok(());
    }

    if cache_dir == Path::new("/") {
        anyhow::bail!("refusing to clean cache_dir=/");
    }

    info!(cache_dir = %cache_dir.display(), "cache clean requested");

    match fs::remove_dir_all(&cache_dir) {
        Ok(()) => {
            println!("Cleaned local cache at {}", cache_dir.display());
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!(
                "Cache already clean: directory {} does not exist",
                cache_dir.display()
            );
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to clean local cache directory {}",
                    cache_dir.display()
                )
            });
        }
    }

    Ok(())
}

pub(super) fn run_cache_prune(cwd: &Path, config: &Config, yes: bool) -> anyhow::Result<()> {
    let cache_root = if config.cache_dir.is_absolute() {
        config.cache_dir.clone()
    } else {
        cwd.join(&config.cache_dir)
    };

    if cache_root == Path::new("/") {
        anyhow::bail!("refusing to prune cache_dir=/");
    }

    info!(cache_dir = %cache_root.display(), dry_run = !yes, "cache prune requested");

    let reconcile = index::reconcile_global_index(&cache_root).with_context(|| {
        format!(
            "failed to reconcile global ref index under {}",
            cache_root.display()
        )
    })?;

    let checkout_candidates =
        collect_prunable_checkouts(&cache_root.join("sources"), &reconcile.live_cache_keys)?;
    let mirror_candidates =
        collect_prunable_mirrors(&cache_root.join("repos"), &reconcile.live_mirror_refs)?;

    println!(
        "Prune scan: stale_project_refs_removed={} stale_index_entries_removed={} index_updated={} checkout_candidates={} mirror_candidates={}",
        reconcile.stale_project_references_removed,
        reconcile.empty_entries_removed,
        reconcile.index_updated,
        checkout_candidates.len(),
        mirror_candidates.len()
    );

    for candidate in &checkout_candidates {
        println!(
            "  checkout {} -> {}",
            describe_checkout_candidate(candidate),
            candidate.path.display()
        );
    }
    for candidate in &mirror_candidates {
        println!(
            "  mirror {} -> {}",
            describe_mirror_candidate(candidate),
            candidate.path.display()
        );
    }

    if !yes {
        println!(
            "No-op: pass --yes to prune local cache entries under {}",
            cache_root.display()
        );
        return Ok(());
    }

    let mut removed_checkouts = 0usize;
    for candidate in &checkout_candidates {
        match fs::remove_dir_all(&candidate.path) {
            Ok(()) => removed_checkouts += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to remove prunable checkout {}",
                        candidate.path.display()
                    )
                });
            }
        }
    }

    let mut removed_mirrors = 0usize;
    for candidate in &mirror_candidates {
        match fs::remove_dir_all(&candidate.path) {
            Ok(()) => removed_mirrors += 1,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to remove prunable mirror {}",
                        candidate.path.display()
                    )
                });
            }
        }
    }

    println!(
        "Prune completed: removed_checkouts={} removed_mirrors={} retained_checkouts={} retained_mirrors={}",
        removed_checkouts,
        removed_mirrors,
        checkout_candidates.len().saturating_sub(removed_checkouts),
        mirror_candidates.len().saturating_sub(removed_mirrors)
    );

    Ok(())
}

fn collect_prunable_checkouts(
    sources_root: &Path,
    live_cache_keys: &std::collections::BTreeSet<String>,
) -> anyhow::Result<Vec<PrunableCheckout>> {
    let mut checkout_paths = Vec::new();
    collect_checkout_dirs(sources_root, &mut checkout_paths)?;

    let mut candidates = Vec::new();
    for path in checkout_paths {
        let Some(cache_key) = checkout_path_to_cache_key(sources_root, &path) else {
            continue;
        };
        if !live_cache_keys.contains(&cache_key) {
            candidates.push(PrunableCheckout { path, cache_key });
        }
    }

    candidates.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path));
    Ok(candidates)
}

fn collect_checkout_dirs(root: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(root)
        .with_context(|| format!("failed to read sources directory {}", root.display()))?
    {
        let entry = entry.with_context(|| {
            format!(
                "failed to read entry from sources directory {}",
                root.display()
            )
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to inspect path {}", path.display()))?;

        if !metadata.is_dir() {
            continue;
        }

        if path.join(".git").exists() {
            out.push(path);
            continue;
        }

        collect_checkout_dirs(&path, out)?;
    }

    Ok(())
}

fn checkout_path_to_cache_key(sources_root: &Path, checkout_path: &Path) -> Option<String> {
    let relative = checkout_path.strip_prefix(sources_root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        parts.push(component.as_os_str().to_string_lossy().to_string());
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

fn collect_prunable_mirrors(
    repos_root: &Path,
    live_mirror_refs: &std::collections::BTreeSet<index::MirrorRef>,
) -> anyhow::Result<Vec<PrunableMirror>> {
    let mut candidates = Vec::new();
    if !repos_root.exists() {
        return Ok(candidates);
    }

    for ecosystem_entry in fs::read_dir(repos_root)
        .with_context(|| format!("failed to read repos directory {}", repos_root.display()))?
    {
        let ecosystem_entry = ecosystem_entry.with_context(|| {
            format!(
                "failed to read entry from repos directory {}",
                repos_root.display()
            )
        })?;
        let ecosystem_path = ecosystem_entry.path();
        let ecosystem_metadata = fs::symlink_metadata(&ecosystem_path).with_context(|| {
            format!(
                "failed to inspect ecosystem path in repos {}",
                ecosystem_path.display()
            )
        })?;
        if !ecosystem_metadata.is_dir() {
            continue;
        }
        let ecosystem = ecosystem_entry.file_name().to_string_lossy().to_string();

        for mirror_entry in fs::read_dir(&ecosystem_path).with_context(|| {
            format!(
                "failed to read mirror directory for ecosystem {}",
                ecosystem_path.display()
            )
        })? {
            let mirror_entry = mirror_entry.with_context(|| {
                format!(
                    "failed to read entry from mirror directory {}",
                    ecosystem_path.display()
                )
            })?;
            let mirror_path = mirror_entry.path();
            let mirror_metadata = fs::symlink_metadata(&mirror_path).with_context(|| {
                format!("failed to inspect mirror path {}", mirror_path.display())
            })?;
            if !mirror_metadata.is_dir() {
                continue;
            }

            let file_name = mirror_entry.file_name().to_string_lossy().to_string();
            let Some(normalized_locator) = file_name.strip_suffix(".git") else {
                continue;
            };

            let mirror_ref = index::MirrorRef {
                ecosystem: ecosystem.clone(),
                normalized_locator: normalized_locator.to_string(),
            };
            if !live_mirror_refs.contains(&mirror_ref) {
                candidates.push(PrunableMirror {
                    path: mirror_path,
                    ecosystem: ecosystem.clone(),
                    normalized_locator: normalized_locator.to_string(),
                });
            }
        }
    }

    candidates.sort_by(|lhs, rhs| lhs.path.cmp(&rhs.path));
    Ok(candidates)
}

#[derive(Clone, Debug)]
struct PrunableCheckout {
    path: PathBuf,
    cache_key: String,
}

#[derive(Clone, Debug)]
struct PrunableMirror {
    path: PathBuf,
    ecosystem: String,
    normalized_locator: String,
}

fn describe_checkout_candidate(candidate: &PrunableCheckout) -> String {
    let parts = candidate.cache_key.split('/').collect::<Vec<_>>();
    if parts.len() < 4 {
        return candidate.cache_key.clone();
    }

    let ecosystem = parts[0];
    let normalized_locator = parts[1];
    let source_fingerprint = parts[parts.len() - 1];
    let requested_revision = parts[2..parts.len() - 1].join("/");
    let locator = depspec::denormalize_locator(normalized_locator)
        .unwrap_or_else(|| normalized_locator.to_string());

    format!(
        "{}:{}@{} ({})",
        ecosystem, locator, requested_revision, source_fingerprint
    )
}

fn describe_mirror_candidate(candidate: &PrunableMirror) -> String {
    let locator = depspec::denormalize_locator(&candidate.normalized_locator)
        .unwrap_or_else(|| candidate.normalized_locator.clone());
    format!("{}:{}", candidate.ecosystem, locator)
}
