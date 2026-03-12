use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Context;
use tracing::{info, warn};

use crate::config::Config;
use crate::depspec::{self, Ecosystem, SourceKind};
use crate::index;
use crate::installed_version;
use crate::providers;
use crate::registry_resolver;
use crate::remote_cache;
use crate::source;

#[derive(Clone, Debug)]
pub(super) struct PullTargetResolution {
    pub target: source::GitPullTarget,
    pub aliases: BTreeSet<String>,
    pub registry_refs: BTreeSet<index::RegistrySpecRef>,
}

#[derive(Clone, Debug)]
pub(super) struct PullResolution {
    pub targets: Vec<PullTargetResolution>,
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
        let version_for_key = first.target.requested_revision.as_str();
        let preview_cache_key = depspec::cache_key(
            &first.target.ecosystem,
            &first.target.locator,
            version_for_key,
            "source-fingerprint-pending",
        );
        let preview_link_path = depspec::link_path(
            &first.target.ecosystem,
            &first.target.locator,
            version_for_key,
        );
        info!(
            first_dep_ecosystem = first.target.ecosystem.as_str(),
            first_dep_locator = %first.target.locator,
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

    for (index, target_resolution) in resolved.targets.iter().enumerate() {
        let target = &target_resolution.target;
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

        let link_metadata = index::LinkRecordMetadata {
            aliases: target_resolution.aliases.clone(),
            registry_refs: target_resolution.registry_refs.clone(),
        };

        if let Err(err) = index::record_link_with_metadata(
            cwd,
            &cache_root,
            target,
            &materialized,
            &link_metadata,
        ) {
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
) -> anyhow::Result<Vec<PullTargetResolution>> {
    let normalized_specs = normalize_explicit_dep_specs_for_pull(cwd, dep_specs)?;
    let parsed_specs = super::parse_dep_specs(&normalized_specs)?;
    let mut targets = Vec::new();

    for (original_spec, spec) in normalized_specs.into_iter().zip(parsed_specs.into_iter()) {
        match spec.source_kind {
            SourceKind::Git {
                url,
                requested_revision,
            } => {
                let mut aliases = BTreeSet::new();
                aliases.insert(original_spec);

                let requested_revision = match requested_revision {
                    Some(requested_revision) => requested_revision,
                    None => {
                        let resolved =
                            source::resolve_default_remote_revision(&url).with_context(|| {
                                format!("failed to resolve default revision for {}", url)
                            })?;
                        println!(
                            "resolved {} default branch {} -> {}",
                            url, resolved.default_branch_ref, resolved.commit_id
                        );
                        resolved.commit_id
                    }
                };

                targets.push(PullTargetResolution {
                    target: source::GitPullTarget {
                        ecosystem: spec.ecosystem,
                        locator: url.clone(),
                        git_url: url,
                        requested_revision,
                    },
                    aliases,
                    registry_refs: BTreeSet::new(),
                });
            }
            SourceKind::Registry => {
                let mut spec = spec;
                if spec.ecosystem == Ecosystem::Npm && spec.version.is_none() {
                    if let Some(detected) =
                        installed_version::detect_installed_npm_version(cwd, &spec.locator)?
                    {
                        println!(
                            "detected installed npm version for {}: {} (from {})",
                            spec.locator,
                            detected.version,
                            detected.source.as_str()
                        );
                        spec.version = Some(detected.version);
                    } else {
                        println!(
                            "no installed npm version detected for {}; falling back to registry latest",
                            spec.locator
                        );
                    }
                }
                if spec.ecosystem == Ecosystem::Pypi && spec.version.is_none() {
                    if let Some(detected) =
                        installed_version::detect_installed_pypi_version(cwd, &spec.locator)?
                    {
                        println!(
                            "detected installed pypi version for {}: {} (from {})",
                            spec.locator,
                            detected.version,
                            detected.source.as_str()
                        );
                        spec.version = Some(detected.version);
                    } else {
                        println!(
                            "no installed pypi version detected for {}; falling back to registry latest",
                            spec.locator
                        );
                    }
                }

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

                let mut aliases = BTreeSet::new();
                aliases.insert(original_spec);
                aliases.insert(format!(
                    "{}:{}",
                    resolved.target.ecosystem.as_str(),
                    resolved.target.locator
                ));
                aliases.insert(format!(
                    "{}:{}@{}",
                    resolved.target.ecosystem.as_str(),
                    resolved.target.locator,
                    resolved.package_version
                ));

                let mut registry_refs = BTreeSet::new();
                if let Some(registry_ref) = registry_ref(
                    &resolved.target.ecosystem,
                    &resolved.target.locator,
                    Some(resolved.package_version.clone()),
                ) {
                    registry_refs.insert(registry_ref);
                }

                targets.push(PullTargetResolution {
                    target: resolved.target,
                    aliases,
                    registry_refs,
                });
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

            let ecosystem = ecosystem_from_provider(&dep.ecosystem);
            let mut aliases = BTreeSet::new();
            aliases.insert(format!("{}:{}", ecosystem.as_str(), dep.name));
            aliases.insert(format!(
                "{}:{}@{}",
                ecosystem.as_str(),
                dep.name,
                dep.version
            ));

            let mut registry_refs = BTreeSet::new();
            if let Some(registry_ref) =
                registry_ref(&ecosystem, &dep.name, Some(dep.version.clone()))
            {
                registry_refs.insert(registry_ref);
            }

            targets.push(PullTargetResolution {
                target: source::GitPullTarget {
                    ecosystem,
                    locator: git_hint.url.clone(),
                    git_url: git_hint.url,
                    requested_revision: git_hint.requested_revision,
                },
                aliases,
                registry_refs,
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

fn deduplicate_pull_targets(targets: Vec<PullTargetResolution>) -> Vec<PullTargetResolution> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut deduped: Vec<PullTargetResolution> = Vec::new();

    for target in targets {
        let key = format!(
            "{}||{}||{}",
            target.target.ecosystem.as_str(),
            target.target.git_url,
            target.target.requested_revision
        );

        if let Some(existing_index) = seen.get(&key).copied() {
            if let Some(existing_target) = deduped.get_mut(existing_index) {
                existing_target.aliases.extend(target.aliases);
                existing_target.registry_refs.extend(target.registry_refs);
            }
            continue;
        }

        seen.insert(key, deduped.len());
        deduped.push(target);
    }

    deduped
}

fn registry_ref(
    ecosystem: &Ecosystem,
    name: &str,
    package_version: Option<String>,
) -> Option<index::RegistrySpecRef> {
    let ecosystem = index::RegistrySpecEcosystem::from_depspec_ecosystem(ecosystem)?;
    Some(index::RegistrySpecRef {
        ecosystem,
        name: name.to_string(),
        package_version,
    })
}

fn ecosystem_from_provider(ecosystem: &providers::ProviderEcosystem) -> Ecosystem {
    match ecosystem {
        providers::ProviderEcosystem::Npm => Ecosystem::Npm,
        providers::ProviderEcosystem::Pypi => Ecosystem::Pypi,
    }
}

fn ecosystem_from_provider_kind(kind: &providers::ProviderKind) -> &'static str {
    match kind {
        providers::ProviderKind::Package
        | providers::ProviderKind::Pnpm
        | providers::ProviderKind::Yarn => "npm",
        providers::ProviderKind::Uv => "pypi",
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

    #[test]
    fn shorthand_inference_rewrites_with_single_pnpm_lockfile() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("pnpm-lock.yaml"), "").expect("write pnpm lock");

        let normalized =
            normalize_explicit_dep_specs_for_pull(temp.path(), &[String::from("zod@3.23.8")])
                .expect("normalize shorthand");
        assert_eq!(normalized, vec![String::from("npm:zod@3.23.8")]);
    }

    #[test]
    fn shorthand_inference_rewrites_with_single_yarn_lockfile() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("yarn.lock"), "").expect("write yarn lock");

        let normalized =
            normalize_explicit_dep_specs_for_pull(temp.path(), &[String::from("zod@3.23.8")])
                .expect("normalize shorthand");
        assert_eq!(normalized, vec![String::from("npm:zod@3.23.8")]);
    }
}
