use std::collections::BTreeMap;

use anyhow::Context;
use reqwest::Url;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::depspec::{DepSpec, Ecosystem, SourceKind};
use crate::source::GitPullTarget;

const DEFAULT_NPM_REGISTRY_BASE: &str = "https://registry.npmjs.org";
const DEFAULT_PYPI_REGISTRY_BASE: &str = "https://pypi.org/pypi";

pub struct RegistryResolution {
    pub target: GitPullTarget,
    pub package_version: String,
}

pub fn resolve_registry_spec(spec: DepSpec) -> anyhow::Result<RegistryResolution> {
    match spec.source_kind {
        SourceKind::Git { .. } => {
            anyhow::bail!("resolve_registry_spec called with git source spec");
        }
        SourceKind::Registry => {}
    }

    match spec.ecosystem {
        Ecosystem::Npm => resolve_npm(spec),
        Ecosystem::Pypi => resolve_pypi(spec),
        other => anyhow::bail!(
            "unsupported registry ecosystem '{}' for package-based pull; supported: npm, pypi",
            other.as_str()
        ),
    }
}

fn resolve_npm(spec: DepSpec) -> anyhow::Result<RegistryResolution> {
    let package_name = spec.locator.clone();
    let endpoint = npm_endpoint(&package_name)?;

    let client = Client::builder()
        .user_agent("pkgrep")
        .build()
        .context("failed to initialize HTTP client for npm metadata resolution")?;
    let response = client
        .get(endpoint.clone())
        .send()
        .with_context(|| format!("failed to fetch npm metadata from {}", endpoint))?;
    let response = response
        .error_for_status()
        .with_context(|| format!("npm metadata request failed for package '{}'", package_name))?;
    let metadata: NpmRegistryPackage = response
        .json()
        .with_context(|| format!("failed to parse npm metadata JSON for '{}'", package_name))?;

    let selected_version = match spec.version {
        Some(version) => version,
        None => metadata
            .dist_tags
            .and_then(|dist_tags| dist_tags.get("latest").cloned())
            .ok_or_else(|| {
                anyhow::anyhow!("npm package '{}' has no latest dist-tag", package_name)
            })?,
    };

    let version_entry = metadata
        .versions
        .and_then(|versions| versions.get(&selected_version).cloned())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "npm package '{}' does not contain requested version '{}'",
                package_name,
                selected_version
            )
        })?;

    let repository_url = repository_url_from_field(version_entry.repository.as_ref())
        .or_else(|| repository_url_from_field(metadata.repository.as_ref()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "npm package '{}' does not provide a repository URL for version '{}'",
                package_name,
                selected_version
            )
        })?;
    let git_url = normalize_git_repository_url(&repository_url).ok_or_else(|| {
        anyhow::anyhow!(
            "npm package '{}' repository URL is not a supported git URL: {}",
            package_name,
            repository_url
        )
    })?;

    let requested_revision = version_entry
        .git_head
        .or_else(|| version_entry.dist.and_then(|dist| dist.git_head))
        .unwrap_or_else(|| selected_version.clone());

    Ok(RegistryResolution {
        target: GitPullTarget {
            ecosystem: Ecosystem::Npm,
            locator: package_name,
            git_url: git_url.clone(),
            requested_revision,
        },
        package_version: selected_version,
    })
}

fn resolve_pypi(spec: DepSpec) -> anyhow::Result<RegistryResolution> {
    let package_name = spec.locator.clone();
    let endpoint = pypi_endpoint(&package_name)?;

    let client = Client::builder()
        .user_agent("pkgrep")
        .build()
        .context("failed to initialize HTTP client for pypi metadata resolution")?;
    let response = client
        .get(endpoint.clone())
        .send()
        .with_context(|| format!("failed to fetch pypi metadata from {}", endpoint))?;
    let response = response.error_for_status().with_context(|| {
        format!(
            "pypi metadata request failed for package '{}'",
            package_name
        )
    })?;
    let metadata: PypiPackageResponse = response
        .json()
        .with_context(|| format!("failed to parse pypi metadata JSON for '{}'", package_name))?;

    let selected_version = spec.version.unwrap_or(metadata.info.version.clone());

    let repository_url = pypi_repository_url(&metadata.info).ok_or_else(|| {
        anyhow::anyhow!(
            "pypi package '{}' does not provide a repository/source URL in metadata",
            package_name
        )
    })?;
    let git_url = normalize_git_repository_url(&repository_url).ok_or_else(|| {
        anyhow::anyhow!(
            "pypi package '{}' repository URL is not a supported git URL: {}",
            package_name,
            repository_url
        )
    })?;

    Ok(RegistryResolution {
        target: GitPullTarget {
            ecosystem: Ecosystem::Pypi,
            locator: package_name,
            git_url: git_url.clone(),
            requested_revision: selected_version.clone(),
        },
        package_version: selected_version,
    })
}

fn npm_endpoint(package_name: &str) -> anyhow::Result<Url> {
    let base = std::env::var("PKGREP_NPM_REGISTRY_URL")
        .unwrap_or_else(|_| DEFAULT_NPM_REGISTRY_BASE.to_string());
    let mut url =
        Url::parse(&base).with_context(|| format!("invalid npm registry URL: {}", base))?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid npm registry URL path: {}", base))?
        .pop_if_empty()
        .push(package_name);
    Ok(url)
}

fn pypi_endpoint(package_name: &str) -> anyhow::Result<Url> {
    let base = std::env::var("PKGREP_PYPI_REGISTRY_URL")
        .unwrap_or_else(|_| DEFAULT_PYPI_REGISTRY_BASE.to_string());
    let mut url =
        Url::parse(&base).with_context(|| format!("invalid pypi registry URL: {}", base))?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid pypi registry URL path: {}", base))?
        .pop_if_empty()
        .push(package_name)
        .push("json");
    Ok(url)
}

fn repository_url_from_field(field: Option<&RepositoryField>) -> Option<String> {
    match field? {
        RepositoryField::String(raw) => Some(raw.clone()),
        RepositoryField::Object { url } => url.clone(),
    }
}

fn pypi_repository_url(info: &PypiInfo) -> Option<String> {
    let project_urls = info.project_urls.as_ref()?;
    let preferred_keys = ["Source", "Source Code", "Repository", "Code", "Homepage"];
    for key in preferred_keys {
        if let Some(url) = project_urls.get(key) {
            return Some(url.clone());
        }
    }
    project_urls
        .values()
        .next()
        .cloned()
        .or_else(|| info.home_page.clone())
}

fn normalize_git_repository_url(raw: &str) -> Option<String> {
    let mut url = raw.trim().to_string();

    if let Some(stripped) = url.strip_prefix("git+") {
        url = stripped.to_string();
    }
    if let Some(stripped) = url.strip_prefix("github:") {
        url = format!("https://github.com/{stripped}");
    }
    if let Some(stripped) = url.strip_prefix("git@github.com:") {
        url = format!("https://github.com/{stripped}");
    }
    if let Some((prefix, _)) = url.split_once('#') {
        url = prefix.to_string();
    }
    if let Some(stripped) = url.strip_prefix("git://") {
        url = format!("https://{stripped}");
    }

    let supported_scheme = ["https://", "http://", "ssh://"]
        .iter()
        .any(|scheme| url.starts_with(scheme));
    if !supported_scheme {
        return None;
    }

    if !url.ends_with(".git") {
        url.push_str(".git");
    }
    Some(url)
}

#[derive(Debug, Deserialize)]
struct NpmRegistryPackage {
    #[serde(default, rename = "dist-tags")]
    dist_tags: Option<BTreeMap<String, String>>,
    #[serde(default)]
    versions: Option<BTreeMap<String, NpmVersionEntry>>,
    #[serde(default)]
    repository: Option<RepositoryField>,
}

#[derive(Clone, Debug, Deserialize)]
struct NpmVersionEntry {
    #[serde(default)]
    repository: Option<RepositoryField>,
    #[serde(default, rename = "gitHead")]
    git_head: Option<String>,
    #[serde(default)]
    dist: Option<NpmDistEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct NpmDistEntry {
    #[serde(default, rename = "gitHead")]
    git_head: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum RepositoryField {
    String(String),
    Object { url: Option<String> },
}

#[derive(Debug, Deserialize)]
struct PypiPackageResponse {
    info: PypiInfo,
}

#[derive(Debug, Deserialize)]
struct PypiInfo {
    version: String,
    #[serde(default)]
    project_urls: Option<BTreeMap<String, String>>,
    #[serde(default)]
    home_page: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_github_shorthand_url() {
        let raw = "github:colinhacks/zod";
        let url = normalize_git_repository_url(raw).expect("normalized");
        assert_eq!(url, "https://github.com/colinhacks/zod.git");
    }

    #[test]
    fn normalizes_git_plus_url_and_fragment() {
        let raw = "git+https://github.com/axios/axios.git#v1.7.0";
        let url = normalize_git_repository_url(raw).expect("normalized");
        assert_eq!(url, "https://github.com/axios/axios.git");
    }

    #[test]
    fn prefers_pypi_source_project_url() {
        let mut project_urls = BTreeMap::new();
        project_urls.insert("Homepage".to_string(), "https://example.com".to_string());
        project_urls.insert(
            "Source".to_string(),
            "https://github.com/psf/requests".to_string(),
        );
        let info = PypiInfo {
            version: "2.32.3".to_string(),
            project_urls: Some(project_urls),
            home_page: Some("https://requests.readthedocs.io".to_string()),
        };

        let url = pypi_repository_url(&info).expect("source url");
        assert_eq!(url, "https://github.com/psf/requests");
    }
}
