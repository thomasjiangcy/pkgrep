use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ecosystem {
    Npm,
    Pypi,
    Git,
    Other(String),
}

impl Ecosystem {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Npm => "npm",
            Self::Pypi => "pypi",
            Self::Git => "git",
            Self::Other(scheme) => scheme.as_str(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Registry,
    Git {
        url: String,
        requested_revision: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepSpec {
    pub ecosystem: Ecosystem,
    pub locator: String,
    pub version: Option<String>,
    pub source_kind: SourceKind,
}

pub fn parse(input: &str) -> Result<DepSpec, String> {
    let (scheme, rest) = input
        .split_once(':')
        .ok_or_else(|| format!("invalid dep spec '{input}': missing '<scheme>:' prefix"))?;

    if scheme.is_empty() {
        return Err(format!(
            "invalid dep spec '{input}': scheme must not be empty"
        ));
    }

    if rest.is_empty() {
        return Err(format!(
            "invalid dep spec '{input}': locator must not be empty"
        ));
    }

    if scheme == "git" {
        return parse_git(input, rest);
    }

    let ecosystem = match scheme {
        "npm" => Ecosystem::Npm,
        "pypi" => Ecosystem::Pypi,
        other => Ecosystem::Other(other.to_string()),
    };

    let (locator, version) = match rest.rsplit_once('@') {
        Some((loc, ver)) if !loc.is_empty() && !ver.is_empty() => {
            (loc.to_string(), Some(ver.to_string()))
        }
        Some((_loc, "")) => {
            return Err(format!(
                "invalid dep spec '{input}': version marker '@' present but version is empty"
            ));
        }
        _ => (rest.to_string(), None),
    };

    Ok(DepSpec {
        ecosystem,
        locator,
        version,
        source_kind: SourceKind::Registry,
    })
}

fn parse_git(input: &str, rest: &str) -> Result<DepSpec, String> {
    let (url, requested_revision) =
        split_git_locator_and_revision(rest).ok_or_else(|| {
            format!(
                "invalid dep spec '{input}': git specs must include a revision via '@<revision>' or '#<revision>' (for example git:https://github.com/org/repo.git@<rev> or git:https://github.com/org/repo.git#<rev>)"
            )
        })?;

    if url.is_empty() {
        return Err(format!(
            "invalid dep spec '{input}': git URL must not be empty"
        ));
    }
    if requested_revision.is_empty() {
        return Err(format!(
            "invalid dep spec '{input}': git revision must not be empty"
        ));
    }

    Ok(DepSpec {
        ecosystem: Ecosystem::Git,
        locator: url.to_string(),
        version: Some(requested_revision.to_string()),
        source_kind: SourceKind::Git {
            url: url.to_string(),
            requested_revision: requested_revision.to_string(),
        },
    })
}

fn split_git_locator_and_revision(rest: &str) -> Option<(&str, &str)> {
    if let Some((url, revision)) = rest.rsplit_once('#')
        && !url.is_empty()
        && !revision.is_empty()
    {
        return Some((url, revision));
    }

    // Keep existing git:<url>@<revision> compatibility, but support revisions
    // containing '@' when the URL ends with ".git".
    if let Some(idx) = rest.find(".git@") {
        let split_at = idx + ".git".len();
        let url = &rest[..split_at];
        let revision = rest.get(split_at + 1..).unwrap_or_default();
        if !url.is_empty() && !revision.is_empty() {
            return Some((url, revision));
        }
    }

    if let Some((url, revision)) = rest.rsplit_once('@')
        && !url.is_empty()
        && !revision.is_empty()
    {
        return Some((url, revision));
    }

    None
}

pub fn normalize_locator(raw: &str) -> String {
    format!("b64_{}", URL_SAFE_NO_PAD.encode(raw.as_bytes()))
}

pub fn denormalize_locator(normalized: &str) -> Option<String> {
    let encoded = normalized.strip_prefix("b64_")?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded.as_bytes()).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn cache_key(
    ecosystem: &Ecosystem,
    locator: &str,
    version: &str,
    source_fingerprint: &str,
) -> String {
    format!(
        "{}/{}/{}/{}",
        ecosystem.as_str(),
        normalize_locator(locator),
        version,
        source_fingerprint
    )
}

pub fn link_path(ecosystem: &Ecosystem, locator: &str, version: &str) -> PathBuf {
    let (parent_components, leaf_component) = split_locator_for_link(locator);
    let mut path = PathBuf::from(".pkgrep")
        .join("deps")
        .join(ecosystem.as_str());
    for component in parent_components {
        path.push(component);
    }
    path.join(format!(
        "{}@{}",
        leaf_component,
        sanitize_version_component(version)
    ))
}

pub fn link_path_prefix(ecosystem: &Ecosystem, locator: &str) -> PathBuf {
    let (parent_components, leaf_component) = split_locator_for_link(locator);
    let mut path = PathBuf::from(".pkgrep")
        .join("deps")
        .join(ecosystem.as_str());
    for component in parent_components {
        path.push(component);
    }
    path.join(format!("{leaf_component}@"))
}

fn split_locator_for_link(locator: &str) -> (Vec<String>, String) {
    let mut components = locator_path_components(locator);
    if components.is_empty() {
        return (Vec::new(), "_".to_string());
    }
    let leaf_component = components.pop().unwrap_or_else(|| "_".to_string());
    (components, leaf_component)
}

fn locator_path_components(locator: &str) -> Vec<String> {
    let raw_components = if let Some((_, rest)) = locator.split_once("://") {
        rest.split('/').collect::<Vec<_>>()
    } else if let Some((lhs, rhs)) = split_scp_like_locator(locator) {
        let host = lhs.rsplit_once('@').map(|(_, host)| host).unwrap_or(lhs);
        let mut parts = vec![host];
        parts.extend(rhs.split('/'));
        parts
    } else {
        locator.split('/').collect::<Vec<_>>()
    };

    raw_components
        .into_iter()
        .filter(|component| !component.is_empty())
        .map(|component| sanitize_locator_component(component, false))
        .collect()
}

fn split_scp_like_locator(locator: &str) -> Option<(&str, &str)> {
    if locator.contains("://") {
        return None;
    }
    let (lhs, rhs) = locator.split_once(':')?;
    if lhs.is_empty() || rhs.is_empty() || !lhs.contains('@') {
        return None;
    }
    Some((lhs, rhs))
}

fn sanitize_version_component(raw: &str) -> String {
    sanitize_locator_component(raw, true)
}

fn sanitize_locator_component(raw: &str, allow_at: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut previous_was_dash = false;

    for ch in raw.chars() {
        let keep = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '-' | '_' | '+')
            || (allow_at && ch == '@');

        if keep {
            out.push(ch);
            previous_was_dash = false;
        } else if !previous_was_dash {
            out.push('-');
            previous_was_dash = true;
        }
    }

    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "_".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn parse_registry_spec_with_version() {
        let spec = parse("npm:react@18.3.1").expect("parse");
        assert_eq!(spec.ecosystem, Ecosystem::Npm);
        assert_eq!(spec.locator, "react");
        assert_eq!(spec.version.as_deref(), Some("18.3.1"));
        assert!(matches!(spec.source_kind, SourceKind::Registry));
    }

    #[test]
    fn parse_git_spec_requires_revision() {
        let err = parse("git:https://github.com/org/repo.git").expect_err("expected failure");
        assert!(err.contains("must include a revision"));
    }

    #[test]
    fn parse_git_spec() {
        let spec = parse("git:https://github.com/org/repo.git@a1b2c3").expect("parse");
        assert_eq!(spec.ecosystem, Ecosystem::Git);
        assert_eq!(spec.locator, "https://github.com/org/repo.git");
        assert_eq!(spec.version.as_deref(), Some("a1b2c3"));
        assert!(matches!(
            spec.source_kind,
            SourceKind::Git {
                ref url,
                ref requested_revision
            } if url == "https://github.com/org/repo.git" && requested_revision == "a1b2c3"
        ));
    }

    #[test]
    fn parse_git_spec_with_hash_separator() {
        let spec = parse("git:https://github.com/org/repo.git#release@2026.02").expect("parse");
        assert_eq!(spec.ecosystem, Ecosystem::Git);
        assert_eq!(spec.locator, "https://github.com/org/repo.git");
        assert_eq!(spec.version.as_deref(), Some("release@2026.02"));
        assert!(matches!(
            spec.source_kind,
            SourceKind::Git {
                ref url,
                ref requested_revision
            } if url == "https://github.com/org/repo.git" && requested_revision == "release@2026.02"
        ));
    }

    #[test]
    fn parse_git_spec_with_revision_containing_at() {
        let spec =
            parse("git:https://github.com/openworkflowdev/openworkflow.git@openworkflow@0.7.3")
                .expect("parse");
        assert_eq!(spec.ecosystem, Ecosystem::Git);
        assert_eq!(
            spec.locator,
            "https://github.com/openworkflowdev/openworkflow.git"
        );
        assert_eq!(spec.version.as_deref(), Some("openworkflow@0.7.3"));
        assert!(matches!(
            spec.source_kind,
            SourceKind::Git {
                ref url,
                ref requested_revision
            } if url == "https://github.com/openworkflowdev/openworkflow.git"
                && requested_revision == "openworkflow@0.7.3"
        ));
    }

    #[test]
    fn normalize_and_denormalize_roundtrip() {
        let locator = "https://github.com/openworkflowdev/openworkflow.git";
        let normalized = normalize_locator(locator);
        let decoded = denormalize_locator(&normalized).expect("decode locator");
        assert_eq!(decoded, locator);
    }

    #[test]
    fn link_path_for_git_url_is_human_readable() {
        let path = link_path(
            &Ecosystem::Git,
            "https://github.com/openworkflowdev/openworkflow.git",
            "openworkflow@0.7.3",
        );
        assert_eq!(
            path,
            PathBuf::from(
                ".pkgrep/deps/git/github.com/openworkflowdev/openworkflow.git@openworkflow@0.7.3"
            )
        );
    }

    #[test]
    fn link_path_prefix_for_git_url_is_human_readable() {
        let path = link_path_prefix(
            &Ecosystem::Git,
            "https://github.com/openworkflowdev/openworkflow.git",
        );
        assert_eq!(
            path,
            PathBuf::from(".pkgrep/deps/git/github.com/openworkflowdev/openworkflow.git@")
        );
    }

    #[test]
    fn link_path_sanitizes_separator_characters_in_revision() {
        let path = link_path(
            &Ecosystem::Git,
            "https://github.com/openworkflowdev/openworkflow.git",
            "refs/tags/v1.2.3",
        );
        assert_eq!(
            path,
            PathBuf::from(
                ".pkgrep/deps/git/github.com/openworkflowdev/openworkflow.git@refs-tags-v1.2.3"
            )
        );
    }

    proptest! {
        #[test]
        fn normalize_is_deterministic(input in "[ -~]{0,128}") {
            prop_assert_eq!(normalize_locator(&input), normalize_locator(&input));
        }

        #[test]
        fn normalize_is_injective_for_distinct_inputs(a in "[ -~]{0,64}", b in "[ -~]{0,64}") {
            prop_assume!(a != b);
            prop_assert_ne!(normalize_locator(&a), normalize_locator(&b));
        }

        #[test]
        fn cache_keys_are_namespaced_by_ecosystem(locator in "[ -~]{1,64}", version in "[A-Za-z0-9._-]{1,32}", fp in "[A-Za-z0-9._:-]{1,64}") {
            let npm = cache_key(&Ecosystem::Npm, &locator, &version, &fp);
            let pypi = cache_key(&Ecosystem::Pypi, &locator, &version, &fp);
            prop_assert_ne!(npm, pypi);
        }

        #[test]
        fn link_paths_are_namespaced_by_ecosystem(locator in "[ -~]{1,64}", version in "[A-Za-z0-9._-]{1,32}") {
            let npm = link_path(&Ecosystem::Npm, &locator, &version);
            let pypi = link_path(&Ecosystem::Pypi, &locator, &version);
            prop_assert_ne!(npm, pypi);
        }
    }
}
