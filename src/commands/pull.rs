use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Context;
use tracing::{info, warn};

use crate::config::Config;
use crate::depspec::{self, Ecosystem, SourceKind};
use crate::index;
use crate::providers;
use crate::registry_resolver;
use crate::remote_cache;
use crate::source;

#[derive(Clone, Debug)]
pub(super) struct PullResolution {
    pub targets: Vec<source::GitPullTarget>,
    pub discovered_lockfiles: usize,
    pub discovered_dependencies: usize,
    pub skipped_non_git_dependencies: usize,
}

pub(super) fn resolve_pull_resolution(
    cwd: &Path,
    dep_specs: &[String],
) -> anyhow::Result<PullResolution> {
    if dep_specs.is_empty() {
        resolve_pull_targets_from_project(cwd)
    } else {
        Ok(PullResolution {
            targets: resolve_pull_targets_from_specs(cwd, dep_specs)?,
            discovered_lockfiles: 0,
            discovered_dependencies: 0,
            skipped_non_git_dependencies: 0,
        })
    }
}

pub(super) fn run_pull(cwd: &Path, config: &Config, dep_specs: Vec<String>) -> anyhow::Result<()> {
    let resolved = resolve_pull_resolution(cwd, &dep_specs)?;

    if dep_specs.is_empty() {
        if resolved.discovered_lockfiles == 0 {
            warn!(
                cwd = %cwd.display(),
                "pull called without explicit dep specs and no supported lockfiles were detected"
            );
            println!(
                "No-op: no dep specs provided and no supported project lockfiles found in {}",
                cwd.display()
            );
            return Ok(());
        }

        if resolved.targets.is_empty() {
            warn!(
                discovered_lockfiles = resolved.discovered_lockfiles,
                discovered_dependencies = resolved.discovered_dependencies,
                skipped_non_git_dependencies = resolved.skipped_non_git_dependencies,
                "supported lockfiles were found, but no git-backed dependencies were available"
            );
            println!(
                "No-op: detected {} dependency entries from {} lockfile(s), but none had git source hints (git-only mode).",
                resolved.discovered_dependencies, resolved.discovered_lockfiles
            );
            return Ok(());
        }
    }

    if let Some(first) = resolved.targets.first() {
        let version_for_key = first.requested_revision.as_str();
        let preview_cache_key = depspec::cache_key(
            &first.ecosystem,
            &first.locator,
            version_for_key,
            "source-fingerprint-pending",
        );
        let preview_link_path =
            depspec::link_path(&first.ecosystem, &first.locator, version_for_key);
        info!(
            first_dep_ecosystem = first.ecosystem.as_str(),
            first_dep_locator = %first.locator,
            first_dep_version = version_for_key,
            first_dep_cache_key_preview = %preview_cache_key,
            first_dep_link_path_preview = %preview_link_path.display(),
            "derived dependency identity preview"
        );
    }

    info!(
        dep_spec_count = dep_specs.len(),
        pull_target_count = resolved.targets.len(),
        discovered_lockfiles = resolved.discovered_lockfiles,
        discovered_dependencies = resolved.discovered_dependencies,
        skipped_non_git_dependencies = resolved.skipped_non_git_dependencies,
        "pull requested"
    );

    let remote_cache_client = remote_cache::RemoteCacheClient::from_config(config)?;
    let cache_root = source::cache_root_for(cwd, &config.cache_dir);

    let mut hydrated_from_remote = 0usize;
    let mut resolved_via_git = 0usize;
    let mut fetched_from_git = 0usize;
    let mut published_to_remote = 0usize;
    let total_targets = resolved.targets.len();

    for (index, target) in resolved.targets.iter().enumerate() {
        println!(
            "[{}/{}] pull {}@{}",
            index + 1,
            total_targets,
            target.git_url,
            target.requested_revision
        );

        let materialized = if let Some(client) = &remote_cache_client {
            println!("  -> checking remote cache");
            match client
                .hydrate_git_source(cwd, config, target)
                .with_context(|| {
                    format!(
                        "failed to hydrate git source {}@{} from remote cache",
                        target.git_url, target.requested_revision
                    )
                })? {
                remote_cache::HydrateOutcome::Hydrated(materialized)
                | remote_cache::HydrateOutcome::AlreadyPresent(materialized) => {
                    hydrated_from_remote += 1;
                    println!("  -> hydrated from remote cache");
                    materialized
                }
                remote_cache::HydrateOutcome::NotFound => {
                    println!("  -> remote cache miss; resolving via local git mirror");
                    let materialized = source::materialize_git_source(cwd, config, target)
                        .with_context(|| {
                            format!(
                                "failed to materialize git source {}@{}",
                                target.git_url, target.requested_revision
                            )
                        })?;
                    resolved_via_git += 1;
                    if materialized.git_fetch_performed {
                        fetched_from_git += 1;
                        println!("  -> fetched requested revision from origin");
                    } else {
                        println!("  -> reused requested revision from local mirror");
                    }

                    match client.publish_git_source(target, &materialized) {
                        Ok(()) => {
                            published_to_remote += 1;
                            println!("  -> published to remote cache");
                        }
                        Err(err) => {
                            warn!(
                                git_url = %target.git_url,
                                requested_revision = %target.requested_revision,
                                error = %err,
                                "failed to publish source to remote cache after git fetch"
                            );
                            println!("  -> warning: publish to remote cache failed");
                        }
                    }

                    materialized
                }
            }
        } else {
            println!("  -> resolving via local git mirror");
            let materialized =
                source::materialize_git_source(cwd, config, target).with_context(|| {
                    format!(
                        "failed to materialize git source {}@{}",
                        target.git_url, target.requested_revision
                    )
                })?;
            resolved_via_git += 1;
            if materialized.git_fetch_performed {
                fetched_from_git += 1;
                println!("  -> fetched requested revision from origin");
            } else {
                println!("  -> reused requested revision from local mirror");
            }
            materialized
        };

        if let Err(err) = index::record_link(cwd, &cache_root, target, &materialized) {
            warn!(
                git_url = %target.git_url,
                requested_revision = %target.requested_revision,
                error = %err,
                "failed to update local index files after link"
            );
        }
        println!("  -> linked {}", materialized.project_link_path.display());

        info!(
            git_url = %target.git_url,
            requested_revision = %target.requested_revision,
            source_fingerprint = %materialized.source_fingerprint,
            cache_key = %materialized.cache_key,
            checkout_path = %materialized.checkout_path.display(),
            link_path = %materialized.project_link_path.display(),
            "materialized git source and linked into project"
        );
    }

    println!(
        "Pull completed: total={} hydrated_from_remote={} resolved_via_git={} fetched_from_git={} published_to_remote={}",
        resolved.targets.len(),
        hydrated_from_remote,
        resolved_via_git,
        fetched_from_git,
        published_to_remote
    );

    Ok(())
}

fn resolve_pull_targets_from_specs(
    cwd: &Path,
    dep_specs: &[String],
) -> anyhow::Result<Vec<source::GitPullTarget>> {
    let normalized_specs = normalize_explicit_dep_specs_for_pull(cwd, dep_specs)?;
    let parsed_specs = super::parse_dep_specs(&normalized_specs)?;
    let mut targets = Vec::new();

    for spec in parsed_specs {
        match spec.source_kind {
            SourceKind::Git {
                url,
                requested_revision,
            } => {
                targets.push(source::GitPullTarget {
                    ecosystem: spec.ecosystem,
                    locator: url.clone(),
                    git_url: url,
                    requested_revision,
                });
            }
            SourceKind::Registry => {
                let spec_label = match &spec.version {
                    Some(version) => {
                        format!("{}:{}@{}", spec.ecosystem.as_str(), spec.locator, version)
                    }
                    None => format!("{}:{}", spec.ecosystem.as_str(), spec.locator),
                };
                println!("resolving package metadata for {}", spec_label);
                let resolved = registry_resolver::resolve_registry_spec(spec)?;
                println!(
                    "  -> resolved to {}@{} (package version {})",
                    resolved.target.git_url,
                    resolved.target.requested_revision,
                    resolved.package_version
                );
                targets.push(resolved.target);
            }
        }
    }

    Ok(deduplicate_pull_targets(targets))
}

fn normalize_explicit_dep_specs_for_pull(
    cwd: &Path,
    dep_specs: &[String],
) -> anyhow::Result<Vec<String>> {
    let has_implicit_specs = dep_specs.iter().any(|spec| !has_explicit_scheme(spec));
    if !has_implicit_specs {
        return Ok(dep_specs.to_vec());
    }

    let inferred_ecosystem = infer_default_registry_ecosystem(cwd)?;
    let inferred_scheme = inferred_ecosystem.as_str();

    dep_specs
        .iter()
        .map(|spec| {
            if has_explicit_scheme(spec) {
                return Ok(spec.clone());
            }

            let rewritten = format!("{inferred_scheme}:{spec}");
            println!("inferred shorthand '{}' as '{}'", spec, rewritten);
            Ok(rewritten)
        })
        .collect()
}

fn has_explicit_scheme(spec: &str) -> bool {
    spec.contains(':')
}

fn infer_default_registry_ecosystem(cwd: &Path) -> anyhow::Result<Ecosystem> {
    let inputs = providers::detect_supported_project_files(cwd);
    if inputs.is_empty() {
        anyhow::bail!(
            "cannot infer shorthand dependency ecosystem in {}: no supported lockfiles detected; use explicit specs such as 'npm:<name>' or 'pypi:<name>'",
            cwd.display()
        );
    }

    let mut ecosystems = BTreeSet::new();
    let mut lockfiles = BTreeSet::new();

    for input in inputs {
        lockfiles.insert(
            input
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| input.path.display().to_string()),
        );
        ecosystems.insert(ecosystem_from_provider_kind(&input.provider));
    }

    if ecosystems.len() != 1 {
        let ecosystem_labels = ecosystems.iter().copied().collect::<Vec<_>>().join(", ");
        let lockfile_labels = lockfiles.into_iter().collect::<Vec<_>>().join(", ");
        anyhow::bail!(
            "cannot infer shorthand dependency ecosystem in {}: multiple supported lockfile ecosystems detected ({ecosystem_labels}) via [{lockfile_labels}]; use explicit specs such as 'npm:<name>' or 'pypi:<name>'",
            cwd.display()
        );
    }

    match ecosystems.into_iter().next() {
        Some("npm") => Ok(Ecosystem::Npm),
        Some("pypi") => Ok(Ecosystem::Pypi),
        Some(other) => Err(anyhow::anyhow!(
            "unsupported inferred shorthand dependency ecosystem '{other}'"
        )),
        None => Err(anyhow::anyhow!(
            "failed to infer shorthand dependency ecosystem"
        )),
    }
}

fn resolve_pull_targets_from_project(cwd: &Path) -> anyhow::Result<PullResolution> {
    let inputs = providers::detect_supported_project_files(cwd);
    let discovered_lockfiles = inputs.len();
    if inputs.is_empty() {
        return Ok(PullResolution {
            targets: Vec::new(),
            discovered_lockfiles: 0,
            discovered_dependencies: 0,
            skipped_non_git_dependencies: 0,
        });
    }

    let mut targets = Vec::new();
    let mut discovered_dependencies = 0usize;
    let mut skipped_non_git_dependencies = 0usize;

    for input in inputs {
        let deps = providers::parse_provider_input(&input).map_err(|err| {
            anyhow::anyhow!(
                "failed to parse project provider input at {}: {err}",
                input.path.display()
            )
        })?;
        for dep in deps {
            discovered_dependencies += 1;
            let Some(git_hint) = dep.git_hint else {
                skipped_non_git_dependencies += 1;
                continue;
            };

            targets.push(source::GitPullTarget {
                ecosystem: ecosystem_from_provider(&dep.ecosystem),
                locator: git_hint.url.clone(),
                git_url: git_hint.url,
                requested_revision: git_hint.requested_revision,
            });
        }
    }

    Ok(PullResolution {
        targets: deduplicate_pull_targets(targets),
        discovered_lockfiles,
        discovered_dependencies,
        skipped_non_git_dependencies,
    })
}

fn deduplicate_pull_targets(targets: Vec<source::GitPullTarget>) -> Vec<source::GitPullTarget> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();

    for target in targets {
        let key = format!(
            "{}||{}||{}",
            target.ecosystem.as_str(),
            target.git_url,
            target.requested_revision
        );
        if seen.insert(key) {
            deduped.push(target);
        }
    }

    deduped
}

fn ecosystem_from_provider(ecosystem: &providers::ProviderEcosystem) -> Ecosystem {
    match ecosystem {
        providers::ProviderEcosystem::Npm => Ecosystem::Npm,
        providers::ProviderEcosystem::Pypi => Ecosystem::Pypi,
    }
}

fn ecosystem_from_provider_kind(kind: &providers::ProviderKind) -> &'static str {
    match kind {
        providers::ProviderKind::NpmPackageLock => "npm",
        providers::ProviderKind::PythonUvLock => "pypi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_scheme_detection() {
        assert!(has_explicit_scheme("npm:zod"));
        assert!(has_explicit_scheme(
            "git:https://github.com/facebook/react.git@v18.3.1"
        ));
        assert!(!has_explicit_scheme("zod"));
        assert!(!has_explicit_scheme("@types/node"));
    }

    #[test]
    fn shorthand_inference_requires_supported_lockfile() {
        let temp = tempfile::tempdir().expect("tempdir");
        let err = normalize_explicit_dep_specs_for_pull(temp.path(), &[String::from("zod@3.23.8")])
            .expect_err("expected error");
        assert!(
            err.to_string()
                .contains("cannot infer shorthand dependency ecosystem")
        );
        assert!(err.to_string().contains("no supported lockfiles detected"));
    }

    #[test]
    fn shorthand_inference_requires_single_ecosystem() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("package-lock.json"), "{}").expect("write package-lock");
        std::fs::write(temp.path().join("uv.lock"), "").expect("write uv.lock");

        let err = normalize_explicit_dep_specs_for_pull(temp.path(), &[String::from("zod@3.23.8")])
            .expect_err("expected error");
        assert!(
            err.to_string()
                .contains("multiple supported lockfile ecosystems detected")
        );
    }

    #[test]
    fn shorthand_inference_rewrites_with_single_ecosystem() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("package-lock.json"), "{}").expect("write package-lock");

        let normalized =
            normalize_explicit_dep_specs_for_pull(temp.path(), &[String::from("zod@3.23.8")])
                .expect("normalize shorthand");
        assert_eq!(normalized, vec![String::from("npm:zod@3.23.8")]);
    }
}
