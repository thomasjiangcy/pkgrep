use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::providers::{GitSourceHint, NormalizedDependency, ProviderEcosystem, ProviderError};

pub(super) fn parse(path: &Path) -> Result<Vec<NormalizedDependency>, ProviderError> {
    let raw = fs::read_to_string(path).map_err(|source| ProviderError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let lock: PnpmLock = serde_yml::from_str(&raw).map_err(|source| ProviderError::Yaml {
        path: path.to_path_buf(),
        source,
    })?;

    let mut deps: BTreeMap<(String, String), NormalizedDependency> = BTreeMap::new();

    if let Some(packages) = lock.packages {
        for (key, entry) in packages {
            let Some((name, raw_ref)) = parse_package_key(&key) else {
                continue;
            };

            let version = entry
                .version
                .clone()
                .or_else(|| parse_semver_from_ref(raw_ref).map(ToOwned::to_owned))
                .unwrap_or_default();
            if version.is_empty() {
                continue;
            }

            let git_hint = entry
                .resolution
                .as_ref()
                .and_then(parse_git_hint_from_resolution)
                .or_else(|| parse_git_hint_from_version_ref(raw_ref));

            merge_dependency(
                &mut deps,
                NormalizedDependency {
                    ecosystem: ProviderEcosystem::Npm,
                    name: name.to_string(),
                    version,
                    git_hint,
                    repository_url: None,
                },
            );
        }
    }

    if deps.is_empty()
        && let Some(importers) = lock.importers
    {
        for importer in importers.into_values() {
            merge_importer_group(&mut deps, importer.dependencies);
            merge_importer_group(&mut deps, importer.dev_dependencies);
            merge_importer_group(&mut deps, importer.optional_dependencies);
        }
    }

    Ok(deps.into_values().collect())
}

fn merge_importer_group(
    deps: &mut BTreeMap<(String, String), NormalizedDependency>,
    group: Option<BTreeMap<String, PnpmImporterDependency>>,
) {
    let Some(group) = group else {
        return;
    };

    for (name, dependency) in group {
        let raw_ref = dependency.version.as_deref().unwrap_or_default();
        let version = parse_semver_from_ref(raw_ref)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| raw_ref.to_string());
        if version.is_empty() {
            continue;
        }

        merge_dependency(
            deps,
            NormalizedDependency {
                ecosystem: ProviderEcosystem::Npm,
                name,
                version,
                git_hint: parse_git_hint_from_version_ref(raw_ref),
                repository_url: None,
            },
        );
    }
}

fn merge_dependency(
    map: &mut BTreeMap<(String, String), NormalizedDependency>,
    candidate: NormalizedDependency,
) {
    let key = (candidate.name.clone(), candidate.version.clone());
    match map.get(&key) {
        Some(existing) if existing.git_hint.is_some() => {}
        Some(_) | None => {
            map.insert(key, candidate);
        }
    }
}

fn parse_package_key(key: &str) -> Option<(&str, &str)> {
    if key.starts_with('@') {
        let slash_index = key.find('/')?;
        let separator_index = key[slash_index + 1..].find('@')?;
        let split_at = slash_index + 1 + separator_index;
        Some((&key[..split_at], &key[split_at + 1..]))
    } else {
        key.split_once('@')
    }
}

fn parse_git_hint_from_resolution(resolution: &PnpmResolution) -> Option<GitSourceHint> {
    let repo = resolution.repo.as_deref()?;
    let commit = resolution.commit.as_deref()?;
    Some(GitSourceHint {
        url: repo.to_string(),
        requested_revision: commit.to_string(),
    })
}

fn parse_git_hint_from_version_ref(raw_ref: &str) -> Option<GitSourceHint> {
    let trimmed = raw_ref.strip_prefix("git+").unwrap_or(raw_ref);
    let (url, revision) = trimmed.rsplit_once('#')?;
    if url.is_empty() || revision.is_empty() {
        return None;
    }

    Some(GitSourceHint {
        url: url.to_string(),
        requested_revision: revision.to_string(),
    })
}

fn parse_semver_from_ref(raw_ref: &str) -> Option<&str> {
    let candidate = raw_ref.split('(').next().unwrap_or(raw_ref);
    if candidate.is_empty() || candidate.contains(':') || candidate.contains('/') {
        return None;
    }
    Some(candidate)
}

#[derive(Debug, Deserialize)]
struct PnpmLock {
    #[serde(default)]
    importers: Option<BTreeMap<String, PnpmImporter>>,
    #[serde(default)]
    packages: Option<BTreeMap<String, PnpmPackage>>,
}

#[derive(Debug, Deserialize)]
struct PnpmImporter {
    #[serde(default)]
    dependencies: Option<BTreeMap<String, PnpmImporterDependency>>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: Option<BTreeMap<String, PnpmImporterDependency>>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: Option<BTreeMap<String, PnpmImporterDependency>>,
}

#[derive(Debug, Deserialize)]
struct PnpmImporterDependency {
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PnpmPackage {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    resolution: Option<PnpmResolution>,
}

#[derive(Debug, Deserialize)]
struct PnpmResolution {
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    repo: Option<String>,
}
