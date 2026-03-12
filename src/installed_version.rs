use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::providers;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstalledVersionSource {
    NodeModules,
    PackageLock,
    PnpmLock,
    YarnLock,
    PackageJson,
    UvLock,
    CargoLock,
}

impl InstalledVersionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NodeModules => "node_modules",
            Self::PackageLock => "package-lock.json",
            Self::PnpmLock => "pnpm-lock.yaml",
            Self::YarnLock => "yarn.lock",
            Self::PackageJson => "package.json",
            Self::UvLock => "uv.lock",
            Self::CargoLock => "Cargo.lock",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledVersion {
    pub version: String,
    pub source: InstalledVersionSource,
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: BTreeMap<String, String>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct PackageLock {
    #[serde(default)]
    packages: BTreeMap<String, PackageLockPackage>,
    #[serde(default)]
    dependencies: BTreeMap<String, PackageLockDependency>,
}

#[derive(Debug, Deserialize)]
struct PackageLockPackage {
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PackageLockDependency {
    version: String,
}

pub fn detect_installed_npm_version(
    cwd: &Path,
    package_name: &str,
) -> Result<Option<InstalledVersion>> {
    Ok(version_from_node_modules(cwd, package_name)
        .map(|version| InstalledVersion {
            version,
            source: InstalledVersionSource::NodeModules,
        })
        .or_else(|| {
            version_from_package_lock(cwd, package_name).map(|version| InstalledVersion {
                version,
                source: InstalledVersionSource::PackageLock,
            })
        })
        .or_else(|| {
            version_from_pnpm_lock(cwd, package_name).map(|version| InstalledVersion {
                version,
                source: InstalledVersionSource::PnpmLock,
            })
        })
        .or_else(|| {
            version_from_yarn_lock(cwd, package_name).map(|version| InstalledVersion {
                version,
                source: InstalledVersionSource::YarnLock,
            })
        })
        .or_else(|| {
            version_from_package_json(cwd, package_name).map(|version| InstalledVersion {
                version,
                source: InstalledVersionSource::PackageJson,
            })
        }))
}

pub fn detect_installed_pypi_version(
    cwd: &Path,
    package_name: &str,
) -> Result<Option<InstalledVersion>> {
    Ok(
        version_from_uv_lock(cwd, package_name).map(|version| InstalledVersion {
            version,
            source: InstalledVersionSource::UvLock,
        }),
    )
}

pub fn detect_installed_crates_version(
    cwd: &Path,
    package_name: &str,
) -> Result<Option<InstalledVersion>> {
    let lock_path = cwd.join("Cargo.lock");
    if !lock_path.exists() {
        return Ok(None);
    }

    let deps = providers::parse_provider_input(&providers::ProviderInputMatch {
        provider: providers::ProviderKind::Cargo,
        path: lock_path,
    })
    .map_err(|err| anyhow::anyhow!("failed to parse Cargo.lock for crates version detection: {err}"))?;

    let normalized_package_name = normalize_crates_package_name(package_name);
    let versions = deps
        .into_iter()
        .filter(|dep| {
            dep.ecosystem == providers::ProviderEcosystem::Crates
                && normalize_crates_package_name(&dep.name) == normalized_package_name
                && dep.git_hint.is_none()
        })
        .map(|dep| dep.version)
        .collect::<std::collections::BTreeSet<_>>();

    match versions.len() {
        0 => Ok(None),
        1 => Ok(versions.into_iter().next().map(|version| InstalledVersion {
            version,
            source: InstalledVersionSource::CargoLock,
        })),
        _ => {
            let joined = versions.into_iter().collect::<Vec<_>>().join(", ");
            anyhow::bail!(
                "multiple installed crates versions detected for {} in Cargo.lock: {}; use an explicit version",
                package_name,
                joined
            );
        }
    }
}

fn version_from_node_modules(cwd: &Path, package_name: &str) -> Option<String> {
    let package_json_path = cwd
        .join("node_modules")
        .join(package_name)
        .join("package.json");
    let bytes = fs::read(package_json_path).ok()?;
    let parsed = serde_json::from_slice::<PackageJson>(&bytes).ok()?;
    parsed.version.filter(|version| !version.trim().is_empty())
}

fn version_from_package_lock(cwd: &Path, package_name: &str) -> Option<String> {
    let lock_path = cwd.join("package-lock.json");
    let bytes = fs::read(lock_path).ok()?;
    let parsed = serde_json::from_slice::<PackageLock>(&bytes).ok()?;
    let package_key = format!("node_modules/{package_name}");
    parsed
        .packages
        .get(&package_key)
        .and_then(|entry| entry.version.clone())
        .or_else(|| {
            parsed
                .dependencies
                .get(package_name)
                .map(|entry| entry.version.clone())
        })
}

fn version_from_pnpm_lock(cwd: &Path, package_name: &str) -> Option<String> {
    let lock_path = cwd.join("pnpm-lock.yaml");
    let content = fs::read_to_string(lock_path).ok()?;
    let prefixes = [
        format!("{package_name}@"),
        format!("'{package_name}@"),
        format!("\"{package_name}@"),
    ];
    for prefix in prefixes {
        if let Some(idx) = content.find(&prefix) {
            let rest = &content[idx + prefix.len()..];
            let version = rest
                .chars()
                .take_while(|ch| {
                    !matches!(ch, '(' | ')' | ':' | '\'' | '"' | ' ' | '\n' | '\r' | '\t')
                })
                .collect::<String>();
            if !version.is_empty() {
                return Some(version);
            }
        }
    }
    None
}

fn version_from_yarn_lock(cwd: &Path, package_name: &str) -> Option<String> {
    let lock_path = cwd.join("yarn.lock");
    let content = fs::read_to_string(lock_path).ok()?;
    let prefixes = [format!("\"{package_name}@"), format!("{package_name}@")];
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        if !prefixes.iter().any(|prefix| line.starts_with(prefix)) {
            continue;
        }

        while let Some(following_line) = lines.peek().copied() {
            let trimmed = following_line.trim();
            if trimmed.starts_with("version ") {
                let version = trimmed
                    .trim_start_matches("version ")
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !version.is_empty() {
                    return Some(version);
                }
            }

            if !following_line.starts_with(' ') && !following_line.starts_with('\t') {
                break;
            }

            lines.next();
        }
    }

    None
}

fn version_from_package_json(cwd: &Path, package_name: &str) -> Option<String> {
    let package_json_path = cwd.join("package.json");
    let bytes = fs::read(package_json_path).ok()?;
    let parsed = serde_json::from_slice::<PackageJson>(&bytes).ok()?;
    version_from_package_json_maps(&parsed, package_name)
}

fn version_from_package_json_maps(parsed: &PackageJson, package_name: &str) -> Option<String> {
    parsed
        .dependencies
        .get(package_name)
        .or_else(|| parsed.dev_dependencies.get(package_name))
        .or_else(|| parsed.peer_dependencies.get(package_name))
        .or_else(|| parsed.optional_dependencies.get(package_name))
        .and_then(|version| normalize_declared_version(version))
}

fn version_from_uv_lock(cwd: &Path, package_name: &str) -> Option<String> {
    let lock_path = cwd.join("uv.lock");
    if !lock_path.exists() {
        return None;
    }

    let input = providers::ProviderInputMatch {
        provider: providers::ProviderKind::Uv,
        path: lock_path,
    };
    let normalized_package_name = normalize_python_package_name(package_name);
    let deps = providers::parse_provider_input(&input).ok()?;

    deps.into_iter().find_map(|dep| {
        if dep.ecosystem != providers::ProviderEcosystem::Pypi {
            return None;
        }

        if normalize_python_package_name(&dep.name) != normalized_package_name {
            return None;
        }

        Some(dep.version)
    })
}

fn normalize_declared_version(version: &str) -> Option<String> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return None;
    }

    let unsupported_prefixes = [
        "workspace:",
        "file:",
        "link:",
        "portal:",
        "patch:",
        "catalog:",
        "npm:",
        "git:",
        "git+",
        "http://",
        "https://",
    ];
    if unsupported_prefixes
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return None;
    }

    let candidate = trimmed.trim_start_matches(['^', '~', '>', '<', '=']);
    if looks_like_exact_semver(candidate) {
        return Some(candidate.to_string());
    }

    None
}

fn looks_like_exact_semver(input: &str) -> bool {
    if input.contains(' ') || input.contains("||") {
        return false;
    }

    let normalized = input.strip_prefix('v').unwrap_or(input);
    let core = normalized
        .split_once('+')
        .map(|(version, _)| version)
        .unwrap_or(normalized);
    let core = core
        .split_once('-')
        .map(|(version, _)| version)
        .unwrap_or(core);

    let mut parts = core.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };

    parts.next().is_none()
        && !major.is_empty()
        && !minor.is_empty()
        && !patch.is_empty()
        && major.chars().all(|ch| ch.is_ascii_digit())
        && minor.chars().all(|ch| ch.is_ascii_digit())
        && patch.chars().all(|ch| ch.is_ascii_digit())
}

fn normalize_python_package_name(input: &str) -> String {
    let mut normalized = String::with_capacity(input.len());
    let mut previous_was_separator = false;

    for ch in input.chars() {
        let is_separator = matches!(ch, '-' | '_' | '.');
        if is_separator {
            if !previous_was_separator {
                normalized.push('-');
            }
            previous_was_separator = true;
            continue;
        }

        normalized.push(ch.to_ascii_lowercase());
        previous_was_separator = false;
    }

    normalized
}

fn normalize_crates_package_name(input: &str) -> String {
    input
        .chars()
        .map(|ch| match ch {
            '_' => '-',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::installed_version::{
        InstalledVersion, InstalledVersionSource, detect_installed_crates_version,
        detect_installed_npm_version, detect_installed_pypi_version,
    };

    #[test]
    fn prefers_node_modules_over_package_json() {
        let temp = tempfile::tempdir().expect("tempdir");
        let node_modules_pkg = temp
            .path()
            .join("node_modules")
            .join("zod")
            .join("package.json");
        fs::create_dir_all(
            node_modules_pkg
                .parent()
                .expect("node_modules package parent exists"),
        )
        .expect("create node_modules package dir");
        fs::write(
            &node_modules_pkg,
            r#"{"version":"3.22.4","dependencies":{"zod":"^3.21.0"}}"#,
        )
        .expect("write node_modules package.json");
        fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"zod":"^3.21.0"}}"#,
        )
        .expect("write package.json");

        let version = detect_installed_npm_version(temp.path(), "zod").expect("detect version");
        assert_eq!(
            version.map(|detected| detected.version),
            Some("3.22.4".to_string())
        );
    }

    #[test]
    fn reads_package_lock_v3_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("package-lock.json"),
            r#"{"packages":{"node_modules/zod":{"version":"3.23.8"}}}"#,
        )
        .expect("write package lock");

        let version = detect_installed_npm_version(temp.path(), "zod").expect("detect version");
        assert_eq!(
            version.map(|detected| detected.version),
            Some("3.23.8".to_string())
        );
    }

    #[test]
    fn reads_pnpm_lock_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("pnpm-lock.yaml"),
            "packages:\n  zod@3.24.1:\n    resolution: {}\n",
        )
        .expect("write pnpm lock");

        let version = detect_installed_npm_version(temp.path(), "zod").expect("detect version");
        assert_eq!(
            version.map(|detected| detected.version),
            Some("3.24.1".to_string())
        );
    }

    #[test]
    fn reads_yarn_lock_entry() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("yarn.lock"),
            "\"zod@^3.0.0\":\n  version \"3.25.0\"\n",
        )
        .expect("write yarn lock");

        let version = detect_installed_npm_version(temp.path(), "zod").expect("detect version");
        assert_eq!(
            version.map(|detected| detected.version),
            Some("3.25.0".to_string())
        );
    }

    #[test]
    fn normalizes_package_json_declared_versions() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"zod":"^3.26.0"},"optionalDependencies":{"chalk":"~5.4.0"}}"#,
        )
        .expect("write package.json");

        let zod = detect_installed_npm_version(temp.path(), "zod").expect("detect zod");
        let chalk = detect_installed_npm_version(temp.path(), "chalk").expect("detect chalk");

        assert_eq!(
            zod.map(|detected| detected.version),
            Some("3.26.0".to_string())
        );
        assert_eq!(
            chalk.map(|detected| detected.version),
            Some("5.4.0".to_string())
        );
    }

    #[test]
    fn ignores_non_exact_package_json_sources() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies":{"zod":"workspace:*","chalk":"github:chalk/chalk"}}"#,
        )
        .expect("write package.json");

        let zod = detect_installed_npm_version(temp.path(), "zod").expect("detect zod");
        let chalk = detect_installed_npm_version(temp.path(), "chalk").expect("detect chalk");

        assert!(zod.is_none());
        assert!(chalk.is_none());
    }

    #[test]
    fn reads_uv_lock_entry_for_pypi() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("uv.lock"),
            r#"
version = 1

[[package]]
name = "requests"
version = "2.32.3"
"#,
        )
        .expect("write uv lock");

        let version =
            detect_installed_pypi_version(temp.path(), "requests").expect("detect pypi version");
        assert_eq!(
            version,
            Some(InstalledVersion {
                version: "2.32.3".to_string(),
                source: InstalledVersionSource::UvLock,
            })
        );
    }

    #[test]
    fn normalizes_python_package_name_for_uv_lock_lookup() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("uv.lock"),
            r#"
version = 1

[[package]]
name = "charset-normalizer"
version = "3.4.2"
"#,
        )
        .expect("write uv lock");

        let version = detect_installed_pypi_version(temp.path(), "charset_normalizer")
            .expect("detect normalized pypi version");
        assert_eq!(
            version,
            Some(InstalledVersion {
                version: "3.4.2".to_string(),
                source: InstalledVersionSource::UvLock,
            })
        );
    }

    #[test]
    fn detects_single_crates_version_from_cargo_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("Cargo.lock"),
            "version = 3\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.228\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        )
        .expect("write cargo lock");

        let version = detect_installed_crates_version(temp.path(), "serde").expect("detect serde");
        assert_eq!(
            version,
            Some(InstalledVersion {
                version: "1.0.228".to_string(),
                source: InstalledVersionSource::CargoLock,
            })
        );
    }

    #[test]
    fn rejects_ambiguous_crates_versions_from_cargo_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("Cargo.lock"),
            "version = 3\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.188\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.228\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        )
        .expect("write cargo lock");

        let err = detect_installed_crates_version(temp.path(), "serde")
            .expect_err("expected ambiguous versions to fail");
        assert!(
            err.to_string()
                .contains("multiple installed crates versions detected for serde in Cargo.lock")
        );
    }

    #[test]
    fn normalizes_crates_package_name_for_cargo_lock_lookup() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("Cargo.lock"),
            "version = 3\n\n[[package]]\nname = \"tokio-util\"\nversion = \"0.7.16\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        )
        .expect("write cargo lock");

        let version = detect_installed_crates_version(temp.path(), "tokio_util")
            .expect("detect normalized crates version");
        assert_eq!(
            version,
            Some(InstalledVersion {
                version: "0.7.16".to_string(),
                source: InstalledVersionSource::CargoLock,
            })
        );
    }
}
