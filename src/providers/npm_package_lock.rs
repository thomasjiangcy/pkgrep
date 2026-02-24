use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::{GitSourceHint, NormalizedDependency, ProviderEcosystem, ProviderError};

pub(super) fn parse(path: &Path) -> Result<Vec<NormalizedDependency>, ProviderError> {
    let raw = fs::read_to_string(path).map_err(|source| ProviderError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let lock: NpmPackageLock =
        serde_json::from_str(&raw).map_err(|source| ProviderError::Json {
            path: path.to_path_buf(),
            source,
        })?;

    let mut deps: BTreeMap<(String, String), NormalizedDependency> = BTreeMap::new();

    if let Some(packages) = lock.packages {
        for (key, entry) in packages {
            if key.is_empty() {
                if let Some(root_deps) = entry.dependencies {
                    for (name, version) in root_deps {
                        let dep = NormalizedDependency {
                            ecosystem: ProviderEcosystem::Npm,
                            name: name.clone(),
                            version,
                            git_hint: None,
                            repository_url: None,
                        };
                        deps.entry((name, dep.version.clone())).or_insert(dep);
                    }
                }
                continue;
            }

            let Some(version) = entry.version else {
                continue;
            };

            let Some(name) = package_name_from_lock_key(&key) else {
                continue;
            };

            let git_hint = entry
                .resolved
                .as_deref()
                .and_then(parse_git_hint_from_npm_resolved);

            merge_dependency(
                &mut deps,
                NormalizedDependency {
                    ecosystem: ProviderEcosystem::Npm,
                    name,
                    version,
                    git_hint,
                    repository_url: None,
                },
            );
        }
    }

    if deps.is_empty()
        && let Some(top_level_deps) = lock.dependencies
    {
        for (name, dep) in top_level_deps {
            let version = dep.version().or(dep.raw_value()).unwrap_or_default();
            if version.is_empty() {
                continue;
            }
            merge_dependency(
                &mut deps,
                NormalizedDependency {
                    ecosystem: ProviderEcosystem::Npm,
                    name,
                    version,
                    git_hint: dep.resolved().and_then(parse_git_hint_from_npm_resolved),
                    repository_url: None,
                },
            );
        }
    }

    Ok(deps.into_values().collect())
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

fn package_name_from_lock_key(key: &str) -> Option<String> {
    let marker = "node_modules/";
    key.rfind(marker)
        .and_then(|idx| key.get(idx + marker.len()..))
        .map(ToOwned::to_owned)
}

fn parse_git_hint_from_npm_resolved(resolved: &str) -> Option<GitSourceHint> {
    let trimmed = resolved.strip_prefix("git+").unwrap_or(resolved);
    let (url, revision) = trimmed.rsplit_once('#')?;
    if url.is_empty() || revision.is_empty() {
        return None;
    }
    Some(GitSourceHint {
        url: url.to_string(),
        requested_revision: revision.to_string(),
    })
}

#[derive(Debug, Deserialize)]
struct NpmPackageLock {
    #[serde(default)]
    packages: Option<BTreeMap<String, NpmPackageEntry>>,
    #[serde(default)]
    dependencies: Option<BTreeMap<String, NpmTopLevelDependency>>,
}

#[derive(Debug, Deserialize)]
struct NpmPackageEntry {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    resolved: Option<String>,
    #[serde(default)]
    dependencies: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NpmTopLevelDependency {
    Object {
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        resolved: Option<String>,
    },
    String(String),
}

impl NpmTopLevelDependency {
    fn version(&self) -> Option<String> {
        match self {
            Self::Object { version, .. } => version.clone(),
            Self::String(raw) => Some(raw.clone()),
        }
    }

    fn resolved(&self) -> Option<&str> {
        match self {
            Self::Object { resolved, .. } => resolved.as_deref(),
            Self::String(_) => None,
        }
    }

    fn raw_value(&self) -> Option<String> {
        match self {
            Self::Object { .. } => None,
            Self::String(raw) => Some(raw.clone()),
        }
    }
}
