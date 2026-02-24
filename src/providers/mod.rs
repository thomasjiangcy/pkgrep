mod npm_package_lock;
mod python_uv_lock;

use std::path::{Path, PathBuf};

use thiserror::Error;

const PACKAGE_LOCK: &str = "package-lock.json";
const UV_LOCK: &str = "uv.lock";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    NpmPackageLock,
    PythonUvLock,
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

    let uv_lock = project_root.join(UV_LOCK);
    if uv_lock.exists() {
        matches.push(ProviderInputMatch {
            provider: ProviderKind::PythonUvLock,
            path: uv_lock,
        });
    }

    matches
}

pub fn parse_provider_input(
    input: &ProviderInputMatch,
) -> Result<Vec<NormalizedDependency>, ProviderError> {
    match input.provider {
        ProviderKind::NpmPackageLock => npm_package_lock::parse(&input.path),
        ProviderKind::PythonUvLock => python_uv_lock::parse(&input.path),
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
    fn detects_project_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join(PACKAGE_LOCK), "{}").expect("write package-lock");
        std::fs::write(temp.path().join(UV_LOCK), "").expect("write uv.lock");

        let detected = detect_supported_project_files(temp.path());
        assert_eq!(detected.len(), 2);
        assert!(
            detected
                .iter()
                .any(|m| matches!(m.provider, ProviderKind::NpmPackageLock))
        );
        assert!(
            detected
                .iter()
                .any(|m| matches!(m.provider, ProviderKind::PythonUvLock))
        );
    }
}
