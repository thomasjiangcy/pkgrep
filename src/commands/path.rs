use std::path::Path;

use crate::depspec::{self, Ecosystem, SourceKind};
use crate::index;

pub(super) fn run_path(cwd: &Path, dep_spec: String) -> anyhow::Result<()> {
    let parsed_specs = super::parse_dep_specs(std::slice::from_ref(&dep_spec))?;
    let spec = parsed_specs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing dependency spec"))?;

    let (locator, requested_revision) = match spec.source_kind {
        SourceKind::Git {
            url,
            requested_revision,
        } => (url, requested_revision),
        SourceKind::Registry => return resolve_registry_path(cwd, &dep_spec, &spec),
    };

    let Some(requested_revision) = requested_revision else {
        return resolve_git_path_without_revision(cwd, &dep_spec, &locator);
    };

    let link_path = cwd.join(depspec::link_path(
        &spec.ecosystem,
        &locator,
        &requested_revision,
    ));

    if link_path.exists() {
        println!("{}", link_path.display());
        return Ok(());
    }

    anyhow::bail!(
        "dependency is not linked in this project: {} (expected path: {})",
        dep_spec,
        link_path.display()
    )
}

fn resolve_git_path_without_revision(
    cwd: &Path,
    dep_spec: &str,
    locator: &str,
) -> anyhow::Result<()> {
    let matches = index::find_git_link_matches(cwd, dep_spec, locator)?;

    match matches.as_slice() {
        [] => anyhow::bail!("dependency is not linked in this project: {}", dep_spec),
        [single_match] => {
            println!("{}", single_match.link_path.display());
            Ok(())
        }
        _ => {
            let mut candidates = matches
                .iter()
                .map(|link_match| link_match.dep_spec.as_str())
                .collect::<Vec<_>>();
            candidates.sort();
            anyhow::bail!(
                "multiple linked dependencies match '{}': {}. Use a versioned dependency spec.",
                dep_spec,
                candidates.join(", ")
            );
        }
    }
}

fn resolve_registry_path(
    cwd: &Path,
    dep_spec: &str,
    spec: &crate::depspec::DepSpec,
) -> anyhow::Result<()> {
    if !matches!(spec.ecosystem, Ecosystem::Npm | Ecosystem::Pypi) {
        anyhow::bail!(
            "path supports npm/pypi registry specs only; use 'npm:<name>[@<version>]' or 'pypi:<name>[@<version>]'"
        );
    }

    let matches = index::find_registry_link_matches(
        cwd,
        dep_spec,
        &spec.ecosystem,
        &spec.locator,
        spec.version.as_deref(),
    )?;

    match matches.as_slice() {
        [] => {
            if spec.version.is_some() {
                anyhow::bail!(
                    "dependency is not linked in this project: {} (if this was linked before metadata support, re-run 'pkgrep pull {}' to backfill)",
                    dep_spec,
                    dep_spec
                );
            }
            anyhow::bail!("dependency is not linked in this project: {}", dep_spec);
        }
        [single_match] => {
            println!("{}", single_match.link_path.display());
            Ok(())
        }
        _ => {
            let mut candidates = matches
                .iter()
                .map(|registry_match| registry_match.dep_spec.as_str())
                .collect::<Vec<_>>();
            candidates.sort();
            let joined_candidates = candidates.join(", ");
            anyhow::bail!(
                "multiple linked dependencies match '{}': {}. Use a versioned dependency spec.",
                dep_spec,
                joined_candidates
            );
        }
    }
}
