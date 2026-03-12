use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::providers::{GitSourceHint, NormalizedDependency, ProviderEcosystem, ProviderError};

pub(super) fn parse(path: &Path) -> Result<Vec<NormalizedDependency>, ProviderError> {
    let raw = fs::read_to_string(path).map_err(|source| ProviderError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let mut deps: BTreeMap<(String, String), NormalizedDependency> = BTreeMap::new();
    let mut selectors: Vec<String> = Vec::new();
    let mut version: Option<String> = None;
    let mut resolved: Option<String> = None;

    for line in raw.lines() {
        if line.trim().is_empty() {
            flush_entry(&mut deps, &selectors, version.take(), resolved.take());
            selectors.clear();
            continue;
        }

        if !line.starts_with(' ') && !line.starts_with('\t') {
            flush_entry(&mut deps, &selectors, version.take(), resolved.take());
            selectors = parse_selectors(line);
            continue;
        }

        let trimmed = line.trim();
        if let Some(parsed_version) = trimmed.strip_prefix("version ") {
            version = Some(unquote(parsed_version));
        } else if let Some(parsed_resolved) = trimmed.strip_prefix("resolved ") {
            resolved = Some(unquote(parsed_resolved));
        }
    }

    flush_entry(&mut deps, &selectors, version, resolved);
    Ok(deps.into_values().collect())
}

fn flush_entry(
    deps: &mut BTreeMap<(String, String), NormalizedDependency>,
    selectors: &[String],
    version: Option<String>,
    resolved: Option<String>,
) {
    let Some(version) = version else {
        return;
    };

    for selector in selectors {
        let Some(name) = selector_name(selector) else {
            continue;
        };
        let git_hint = resolved
            .as_deref()
            .and_then(parse_git_hint)
            .or_else(|| parse_git_hint(selector));
        let dependency = NormalizedDependency {
            ecosystem: ProviderEcosystem::Npm,
            name,
            version: version.clone(),
            git_hint,
            repository_url: None,
        };
        merge_dependency(deps, dependency);
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

fn parse_selectors(line: &str) -> Vec<String> {
    let trimmed = line.trim_end_matches(':');
    trimmed
        .split(", ")
        .map(unquote)
        .filter(|selector| !selector.is_empty())
        .collect()
}

fn selector_name(selector: &str) -> Option<String> {
    if selector.starts_with('@') {
        let slash_index = selector.find('/')?;
        let separator_index = selector[slash_index + 1..].find('@')?;
        let split_at = slash_index + 1 + separator_index;
        return Some(selector[..split_at].to_string());
    }

    selector.split_once('@').map(|(name, _)| name.to_string())
}

fn parse_git_hint(raw: &str) -> Option<GitSourceHint> {
    let trimmed = raw.strip_prefix("git+").unwrap_or(raw);
    let (url, revision) = trimmed.rsplit_once('#')?;
    if url.is_empty() || revision.is_empty() {
        return None;
    }
    Some(GitSourceHint {
        url: url.to_string(),
        requested_revision: revision.to_string(),
    })
}

fn unquote(raw: &str) -> String {
    raw.trim_matches('"').trim_matches('\'').to_string()
}
