use std::path::Path;

use crate::config::Config;
use crate::registry_resolver;

#[derive(Clone, Debug, PartialEq, Eq)]
enum NormalizedAddInput {
    ExplicitDepSpec(String),
    PackageInput(String),
    InferredGitDepSpec { original: String, dep_spec: String },
}

pub fn run_add(cwd: &Path, config: &Config, inputs: Vec<String>) -> anyhow::Result<()> {
    let dep_specs = inputs
        .into_iter()
        .map(|input| normalize_add_input(&input))
        .collect::<anyhow::Result<Vec<_>>>()?;

    for normalized in &dep_specs {
        if let NormalizedAddInput::InferredGitDepSpec { original, dep_spec } = normalized {
            println!("inferred add input '{}' as '{}'", original, dep_spec);
        }
    }

    crate::commands::pull::run_pull(
        cwd,
        config,
        dep_specs
            .into_iter()
            .map(|normalized| match normalized {
                NormalizedAddInput::ExplicitDepSpec(dep_spec)
                | NormalizedAddInput::PackageInput(dep_spec) => dep_spec,
                NormalizedAddInput::InferredGitDepSpec { dep_spec, .. } => dep_spec,
            })
            .collect(),
    )
}

fn normalize_add_input(input: &str) -> anyhow::Result<NormalizedAddInput> {
    if has_explicit_dep_scheme(input) {
        return Ok(NormalizedAddInput::ExplicitDepSpec(input.to_string()));
    }

    if let Some(dep_spec) = normalize_git_repo_input(input) {
        return Ok(NormalizedAddInput::InferredGitDepSpec {
            original: input.to_string(),
            dep_spec,
        });
    }

    if looks_like_url(input) {
        anyhow::bail!(
            "unsupported add input '{}': use an explicit dep spec such as 'git:{}'",
            input,
            input
        );
    }

    Ok(NormalizedAddInput::PackageInput(input.to_string()))
}

fn has_explicit_dep_scheme(input: &str) -> bool {
    input.starts_with("git:") || input.starts_with("npm:") || input.starts_with("pypi:")
}

fn normalize_git_repo_input(input: &str) -> Option<String> {
    if let Some(url) = registry_resolver::normalize_git_repository_url_for_cli(input) {
        return Some(format!("git:{url}"));
    }

    if input.starts_with('@') {
        return None;
    }

    let (owner, repo) = input.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    if !looks_like_github_component(owner) || !looks_like_github_component(repo) {
        return None;
    }

    Some(format!("git:https://github.com/{owner}/{repo}.git"))
}

fn looks_like_url(input: &str) -> bool {
    input.contains("://") || input.starts_with("git@")
}

fn looks_like_github_component(input: &str) -> bool {
    !input.is_empty()
        && input
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_explicit_dep_specs() {
        let normalized =
            normalize_add_input("npm:zod@3.23.8").expect("normalize explicit dep spec");
        assert_eq!(
            normalized,
            NormalizedAddInput::ExplicitDepSpec("npm:zod@3.23.8".to_string())
        );
    }

    #[test]
    fn keeps_package_inputs() {
        let normalized = normalize_add_input("zod").expect("normalize package input");
        assert_eq!(
            normalized,
            NormalizedAddInput::PackageInput("zod".to_string())
        );
    }

    #[test]
    fn keeps_scoped_package_inputs() {
        let normalized = normalize_add_input("@types/node").expect("normalize scoped package");
        assert_eq!(
            normalized,
            NormalizedAddInput::PackageInput("@types/node".to_string())
        );
    }

    #[test]
    fn normalizes_github_owner_repo_input() {
        let normalized = normalize_add_input("vercel/ai").expect("normalize github shorthand");
        assert_eq!(
            normalized,
            NormalizedAddInput::InferredGitDepSpec {
                original: "vercel/ai".to_string(),
                dep_spec: "git:https://github.com/vercel/ai.git".to_string(),
            }
        );
    }

    #[test]
    fn normalizes_generic_https_git_url_input() {
        let normalized =
            normalize_add_input("https://gitlab.com/vercel/ai").expect("normalize https git url");
        assert_eq!(
            normalized,
            NormalizedAddInput::InferredGitDepSpec {
                original: "https://gitlab.com/vercel/ai".to_string(),
                dep_spec: "git:https://gitlab.com/vercel/ai.git".to_string(),
            }
        );
    }

    #[test]
    fn rejects_unsupported_url_scheme_without_explicit_git_scheme() {
        let err = normalize_add_input("ftp://example.com/vercel/ai")
            .expect_err("expected unsupported url to fail");
        assert!(
            err.to_string()
                .contains("unsupported add input 'ftp://example.com/vercel/ai'")
        );
    }
}
