mod npm_package_lock;
mod pnpm_lock;
mod python_uv_lock;
mod yarn_lock;

use std::path::{Path, PathBuf};

use thiserror::Error;

const PACKAGE_LOCK: &str = "package-lock.json";
const PNPM_LOCK: &str = "pnpm-lock.yaml";
const UV_LOCK: &str = "uv.lock";
const YARN_LOCK: &str = "yarn.lock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    NpmPackageLock,
    NpmPnpmLock,
    PythonUvLock,
    NpmYarnLock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderInputMatch {
    pub provider: ProviderKind,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitSourceHint {
    pub url: String,
    pub requested_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderEcosystem {
    Npm,
    Pypi,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedDependency {
    pub ecosystem: ProviderEcosystem,
    pub name: String,
    pub version: String,
    pub git_hint: Option<GitSourceHint>,
    pub repository_url: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("failed to read provider input {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse JSON provider input {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("failed to parse TOML provider input {path}: {source}")]
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("failed to parse YAML provider input {path}: {source}")]
    Yaml {
        path: PathBuf,
        source: serde_yml::Error,
    },
}

pub fn detect_supported_project_files(project_root: &Path) -> Vec<ProviderInputMatch> {
    let mut matches = Vec::new();

    let package_lock = project_root.join(PACKAGE_LOCK);
    if package_lock.exists() {
        matches.push(ProviderInputMatch {
            provider: ProviderKind::NpmPackageLock,
            path: package_lock,
        });
    }

    let pnpm_lock = project_root.join(PNPM_LOCK);
    if pnpm_lock.exists() {
        matches.push(ProviderInputMatch {
            provider: ProviderKind::NpmPnpmLock,
            path: pnpm_lock,
        });
    }

    let uv_lock = project_root.join(UV_LOCK);
    if uv_lock.exists() {
        matches.push(ProviderInputMatch {
            provider: ProviderKind::PythonUvLock,
            path: uv_lock,
        });
    }

    let yarn_lock = project_root.join(YARN_LOCK);
    if yarn_lock.exists() {
        matches.push(ProviderInputMatch {
            provider: ProviderKind::NpmYarnLock,
            path: yarn_lock,
        });
    }

    matches
}

pub fn parse_provider_input(
    input: &ProviderInputMatch,
) -> Result<Vec<NormalizedDependency>, ProviderError> {
    match input.provider {
        ProviderKind::NpmPackageLock => npm_package_lock::parse(&input.path),
        ProviderKind::NpmPnpmLock => pnpm_lock::parse(&input.path),
        ProviderKind::PythonUvLock => python_uv_lock::parse(&input.path),
        ProviderKind::NpmYarnLock => yarn_lock::parse(&input.path),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)
    }

    #[test]
    fn parses_package_lock_fixture() {
        let path = fixture("fixtures/js/package-lock.json");
        let input = ProviderInputMatch {
            provider: ProviderKind::NpmPackageLock,
            path,
        };

        let deps = parse_provider_input(&input).expect("parse package-lock");
        assert!(deps.iter().any(|dep| {
            dep.ecosystem == ProviderEcosystem::Npm
                && dep.name == "react"
                && dep.version == "18.3.1"
        }));
        assert!(deps.iter().any(|dep| {
            dep.name == "demo-git-package"
                && dep.version == "1.0.0"
                && dep.git_hint.as_ref().is_some_and(|hint| {
                    hint.url == "https://example.com/demo-git-package.git"
                        && hint.requested_revision == "abc123def456"
                })
        }));
    }

    #[test]
    fn parses_pnpm_lock_fixture() {
        let path = fixture("fixtures/js/pnpm-lock.yaml");
        let input = ProviderInputMatch {
            provider: ProviderKind::NpmPnpmLock,
            path,
        };

        let deps = parse_provider_input(&input).expect("parse pnpm lock");
        assert!(deps.iter().any(|dep| {
            dep.ecosystem == ProviderEcosystem::Npm
                && dep.name == "react"
                && dep.version == "18.3.1"
        }));
        assert!(deps.iter().any(|dep| {
            dep.name == "demo-git-package"
                && dep.version == "1.0.0"
                && dep.git_hint.as_ref().is_some_and(|hint| {
                    hint.url == "https://example.com/demo-git-package.git"
                        && hint.requested_revision == "abc123def456"
                })
        }));
    }

    #[test]
    fn parses_uv_lock_fixture() {
        let path = fixture("fixtures/python/uv.lock");
        let input = ProviderInputMatch {
            provider: ProviderKind::PythonUvLock,
            path,
        };

        let deps = parse_provider_input(&input).expect("parse uv.lock");
        assert!(deps.iter().any(|dep| {
            dep.ecosystem == ProviderEcosystem::Pypi
                && dep.name == "requests"
                && dep.version == "2.32.3"
        }));
    }

    #[test]
    fn parses_yarn_lock_fixture() {
        let path = fixture("fixtures/js/yarn.lock");
        let input = ProviderInputMatch {
            provider: ProviderKind::NpmYarnLock,
            path,
        };

        let deps = parse_provider_input(&input).expect("parse yarn lock");
        assert!(deps.iter().any(|dep| {
            dep.ecosystem == ProviderEcosystem::Npm
                && dep.name == "react"
                && dep.version == "18.3.1"
        }));
        assert!(deps.iter().any(|dep| {
            dep.name == "demo-git-package"
                && dep.version == "1.0.0"
                && dep.git_hint.as_ref().is_some_and(|hint| {
                    hint.url == "file:///tmp/demo-git-package"
                        && hint.requested_revision == "abc123def456"
                })
        }));
    }

    #[test]
    fn detects_project_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join(PACKAGE_LOCK), "{}").expect("write package-lock");
        std::fs::write(temp.path().join(PNPM_LOCK), "").expect("write pnpm lock");
        std::fs::write(temp.path().join(UV_LOCK), "").expect("write uv.lock");
        std::fs::write(temp.path().join(YARN_LOCK), "").expect("write yarn lock");

        let detected = detect_supported_project_files(temp.path());
        assert_eq!(detected.len(), 4);
        assert!(
            detected
                .iter()
                .any(|m| matches!(m.provider, ProviderKind::NpmPackageLock))
        );
        assert!(
            detected
                .iter()
                .any(|m| matches!(m.provider, ProviderKind::NpmPnpmLock))
        );
        assert!(
            detected
                .iter()
                .any(|m| matches!(m.provider, ProviderKind::PythonUvLock))
        );
        assert!(
            detected
                .iter()
                .any(|m| matches!(m.provider, ProviderKind::NpmYarnLock))
        );
    }
}
