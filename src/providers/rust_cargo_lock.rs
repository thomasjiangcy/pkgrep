use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::providers::{GitSourceHint, NormalizedDependency, ProviderEcosystem, ProviderError};

#[derive(Debug, Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<CargoPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    #[serde(default)]
    source: Option<String>,
}

pub(super) fn parse(path: &Path) -> Result<Vec<NormalizedDependency>, ProviderError> {
    let raw = fs::read_to_string(path).map_err(|source| ProviderError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: CargoLock = toml::from_str(&raw).map_err(|source| ProviderError::Toml {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(parsed
        .package
        .into_iter()
        .map(|package| NormalizedDependency {
            ecosystem: ProviderEcosystem::Crates,
            name: package.name,
            version: package.version,
            git_hint: package
                .source
                .as_deref()
                .and_then(parse_git_hint_from_source),
            repository_url: None,
        })
        .collect())
}

fn parse_git_hint_from_source(source: &str) -> Option<GitSourceHint> {
    let raw = source.strip_prefix("git+")?;
    let (url_with_query, fallback_revision) = raw.rsplit_once('#')?;
    let (url, query) = match url_with_query.split_once('?') {
        Some((url, query)) => (url, Some(query)),
        None => (url_with_query, None),
    };

    Some(GitSourceHint {
        url: url.to_string(),
        requested_revision: query
            .and_then(parse_preferred_revision)
            .unwrap_or_else(|| fallback_revision.to_string()),
    })
}

fn parse_preferred_revision(query: &str) -> Option<String> {
    let mut branch = None;
    let mut tag = None;
    let mut rev = None;

    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        if value.is_empty() {
            continue;
        }

        match key {
            "branch" => branch = Some(value.to_string()),
            "tag" => tag = Some(value.to_string()),
            "rev" => rev = Some(value.to_string()),
            _ => {}
        }
    }

    rev.or(tag).or(branch)
}
