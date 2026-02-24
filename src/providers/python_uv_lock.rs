use std::fs;
use std::path::Path;

use super::{GitSourceHint, NormalizedDependency, ProviderEcosystem, ProviderError};

pub(super) fn parse(path: &Path) -> Result<Vec<NormalizedDependency>, ProviderError> {
    let raw = fs::read_to_string(path).map_err(|source| ProviderError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let value: toml::Value = toml::from_str(&raw).map_err(|source| ProviderError::Toml {
        path: path.to_path_buf(),
        source,
    })?;

    let mut deps = Vec::new();

    let Some(packages) = value.get("package").and_then(toml::Value::as_array) else {
        return Ok(deps);
    };

    for item in packages {
        let Some(table) = item.as_table() else {
            continue;
        };

        let Some(name) = table.get("name").and_then(toml::Value::as_str) else {
            continue;
        };

        let Some(version) = table.get("version").and_then(toml::Value::as_str) else {
            continue;
        };

        let git_hint = table
            .get("source")
            .and_then(toml::Value::as_table)
            .and_then(parse_git_hint_from_uv_source);

        deps.push(NormalizedDependency {
            ecosystem: ProviderEcosystem::Pypi,
            name: name.to_string(),
            version: version.to_string(),
            git_hint,
            repository_url: None,
        });
    }

    Ok(deps)
}

fn parse_git_hint_from_uv_source(
    source: &toml::map::Map<String, toml::Value>,
) -> Option<GitSourceHint> {
    let git_url = source.get("git").and_then(toml::Value::as_str)?;

    let requested_revision = source
        .get("rev")
        .and_then(toml::Value::as_str)
        .or_else(|| source.get("tag").and_then(toml::Value::as_str))
        .or_else(|| source.get("branch").and_then(toml::Value::as_str))
        .unwrap_or("HEAD");

    Some(GitSourceHint {
        url: git_url.to_string(),
        requested_revision: requested_revision.to_string(),
    })
}
