use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use git2::build::CheckoutBuilder;
use git2::{AutotagOption, FetchOptions, Oid, RemoteCallbacks, Repository};
use tracing::debug;

use crate::config::Config;
use crate::depspec::{self, Ecosystem};

#[derive(Clone, Debug)]
pub struct GitPullTarget {
    pub ecosystem: Ecosystem,
    pub locator: String,
    pub git_url: String,
    pub requested_revision: String,
}

#[derive(Clone, Debug)]
pub struct MaterializedSource {
    pub cache_key: String,
    pub source_fingerprint: String,
    pub checkout_path: PathBuf,
    pub project_link_path: PathBuf,
    pub git_fetch_performed: bool,
}

pub fn materialize_git_source(
    cwd: &Path,
    config: &Config,
    target: &GitPullTarget,
) -> anyhow::Result<MaterializedSource> {
    let cache_root = cache_root_for(cwd, &config.cache_dir);

    let mirror_repo_path = mirror_repo_path(&cache_root, &target.ecosystem, &target.git_url);
    let (mirror_repo, git_fetch_performed) = ensure_mirror_repo(
        &target.git_url,
        &mirror_repo_path,
        &target.requested_revision,
    )?;
    let source_fingerprint = resolve_commit_fingerprint(&mirror_repo, &target.requested_revision)?;

    let cache_key = depspec::cache_key(
        &target.ecosystem,
        &target.locator,
        &target.requested_revision,
        &source_fingerprint,
    );
    let checkout_path = cache_root.join("sources").join(&cache_key);
    ensure_checkout_exists(&mirror_repo_path, &checkout_path, &source_fingerprint)?;

    let project_link_path = link_checkout(cwd, target, &checkout_path)?;

    Ok(MaterializedSource {
        cache_key,
        source_fingerprint,
        checkout_path,
        project_link_path,
        git_fetch_performed,
    })
}

pub fn cache_root_for(cwd: &Path, configured_cache_dir: &Path) -> PathBuf {
    if configured_cache_dir.is_absolute() {
        configured_cache_dir.to_path_buf()
    } else {
        cwd.join(configured_cache_dir)
    }
}

pub fn link_checkout(
    cwd: &Path,
    target: &GitPullTarget,
    checkout_path: &Path,
) -> anyhow::Result<PathBuf> {
    let project_link_path = cwd.join(depspec::link_path(
        &target.ecosystem,
        &target.locator,
        &target.requested_revision,
    ));
    ensure_symlink(checkout_path, &project_link_path)?;
    Ok(project_link_path)
}

fn mirror_repo_path(cache_root: &Path, ecosystem: &Ecosystem, git_url: &str) -> PathBuf {
    cache_root
        .join("repos")
        .join(ecosystem.as_str())
        .join(format!("{}.git", depspec::normalize_locator(git_url)))
}

fn ensure_mirror_repo(
    git_url: &str,
    mirror_repo_path: &Path,
    requested_revision: &str,
) -> anyhow::Result<(Repository, bool)> {
    if let Some(parent) = mirror_repo_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create mirror repo parent directory {}",
                parent.display()
            )
        })?;
    }

    let repo = if mirror_repo_path.exists() {
        Repository::open_bare(mirror_repo_path).with_context(|| {
            format!(
                "failed to open existing mirror repo at {}",
                mirror_repo_path.display()
            )
        })?
    } else {
        debug!(
            git_url = %git_url,
            mirror_repo_path = %mirror_repo_path.display(),
            "creating bare mirror repository"
        );
        let repo = Repository::init_bare(mirror_repo_path).with_context(|| {
            format!(
                "failed to initialize bare mirror repo at {}",
                mirror_repo_path.display()
            )
        })?;
        repo.remote("origin", git_url)
            .with_context(|| format!("failed to configure origin remote for {}", git_url))?;
        repo
    };

    let git_fetch_performed = ensure_revision_available(&repo, requested_revision)?;
    Ok((repo, git_fetch_performed))
}

fn ensure_revision_available(repo: &Repository, requested_revision: &str) -> anyhow::Result<bool> {
    if try_resolve_commit_fingerprint_with_alternates(repo, requested_revision).is_some() {
        debug!(
            requested_revision = requested_revision,
            "requested revision already present in mirror repo; skipping fetch"
        );
        return Ok(false);
    }

    fetch_targeted_revision(repo, requested_revision)?;

    if try_resolve_commit_fingerprint_with_alternates(repo, requested_revision).is_none() {
        anyhow::bail!(
            "requested revision '{requested_revision}' is unavailable after targeted fetch"
        );
    }

    Ok(true)
}

fn fetch_targeted_revision(repo: &Repository, requested_revision: &str) -> anyhow::Result<()> {
    let mut remote = repo
        .find_remote("origin")
        .context("failed to find origin remote in mirror repo")?;
    let remote_url = remote.url().unwrap_or("<unknown>").to_string();

    let refspecs = targeted_refspecs(requested_revision);
    let shallow = supports_shallow_fetch(&remote_url);
    debug!(
        remote_url = %remote_url,
        requested_revision = requested_revision,
        refspecs = ?refspecs,
        shallow = shallow,
        "fetching targeted revision from origin"
    );

    let mut errors = Vec::new();
    for refspec in &refspecs {
        let mut fetch_options = fetch_options_with_progress("fetch_targeted", &remote_url, shallow);
        match remote.fetch(&[refspec], Some(&mut fetch_options), None) {
            Ok(()) => {
                if try_resolve_commit_fingerprint_with_alternates(repo, requested_revision)
                    .is_some()
                {
                    return Ok(());
                }
                debug!(
                    remote_url = %remote_url,
                    requested_revision = requested_revision,
                    refspec = refspec,
                    "fetch completed but requested revision is still unresolved; trying next refspec"
                );
            }
            Err(err) => {
                let message = format!("refspec '{refspec}': {err}");
                debug!(
                    remote_url = %remote_url,
                    requested_revision = requested_revision,
                    refspec = refspec,
                    error = %err,
                    "targeted fetch attempt failed"
                );
                errors.push(message);
            }
        }
    }

    anyhow::bail!(
        "failed to fetch requested revision '{}' from {} via targeted refspecs [{}]",
        requested_revision,
        remote_url,
        errors.join("; ")
    )
}

fn fetch_options_with_progress(
    operation: &'static str,
    git_url: &str,
    shallow: bool,
) -> FetchOptions<'static> {
    let mut callbacks = RemoteCallbacks::new();
    let git_url = git_url.to_string();
    let mut last_reported_percent = 0usize;
    callbacks.transfer_progress(move |stats| {
        let total_objects = stats.total_objects();
        if total_objects == 0 {
            return true;
        }

        let received_objects = stats.received_objects();
        let percent = (received_objects.saturating_mul(100)) / total_objects;
        if percent >= last_reported_percent.saturating_add(5) || percent == 100 {
            debug!(
                operation = operation,
                git_url = %git_url,
                received_objects = received_objects,
                total_objects = total_objects,
                indexed_objects = stats.indexed_objects(),
                received_bytes = stats.received_bytes(),
                percent = percent,
                "git transfer progress"
            );
            last_reported_percent = percent;
        }

        true
    });

    let mut options = FetchOptions::new();
    if shallow {
        options.depth(1);
    }
    options.download_tags(AutotagOption::None);
    options.remote_callbacks(callbacks);
    options
}

fn supports_shallow_fetch(remote_url: &str) -> bool {
    let is_local_path = remote_url.starts_with('/')
        || remote_url.starts_with("./")
        || remote_url.starts_with("../")
        || remote_url.starts_with("file://");
    !is_local_path
}

fn targeted_refspecs(requested_revision: &str) -> Vec<String> {
    if requested_revision.starts_with("refs/") {
        return vec![
            format!("{requested_revision}:{requested_revision}"),
            requested_revision.to_string(),
        ];
    }

    if looks_like_hex_revision(requested_revision) {
        return vec![
            requested_revision.to_string(),
            "HEAD:refs/heads/pkgrep-head".to_string(),
            "refs/heads/main:refs/heads/main".to_string(),
            "refs/heads/master:refs/heads/master".to_string(),
        ];
    }

    let mut revisions = vec![requested_revision.to_string()];
    if let Some(alt_revision) = alternate_tag_revision(requested_revision) {
        revisions.push(alt_revision);
    }

    let mut refspecs = Vec::new();
    for revision in revisions {
        push_unique(
            &mut refspecs,
            format!("refs/tags/{revision}:refs/tags/{revision}"),
        );
        push_unique(
            &mut refspecs,
            format!("refs/heads/{revision}:refs/heads/{revision}"),
        );
        push_unique(&mut refspecs, revision);
    }
    refspecs
}

fn alternate_tag_revision(requested_revision: &str) -> Option<String> {
    if !looks_like_semver_revision(requested_revision) {
        return None;
    }

    if let Some(stripped) = requested_revision.strip_prefix('v') {
        if looks_like_plain_semver(stripped) {
            return Some(stripped.to_string());
        }
    } else {
        return Some(format!("v{requested_revision}"));
    }

    None
}

fn looks_like_semver_revision(requested_revision: &str) -> bool {
    let normalized = requested_revision
        .strip_prefix('v')
        .unwrap_or(requested_revision);
    looks_like_plain_semver(normalized)
}

fn looks_like_plain_semver(input: &str) -> bool {
    let mut parts = input.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch_and_suffix) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    if !major.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if !minor.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let patch_core = patch_and_suffix
        .split_once('-')
        .map(|(patch, _)| patch)
        .unwrap_or(patch_and_suffix);
    !patch_core.is_empty() && patch_core.chars().all(|c| c.is_ascii_digit())
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.contains(&value) {
        out.push(value);
    }
}

fn looks_like_hex_revision(requested_revision: &str) -> bool {
    requested_revision.len() >= 7 && requested_revision.chars().all(|c| c.is_ascii_hexdigit())
}

fn try_resolve_commit_fingerprint(repo: &Repository, requested_revision: &str) -> Option<String> {
    let object = repo.revparse_single(requested_revision).ok()?;
    let commit = object.peel_to_commit().ok()?;
    Some(commit.id().to_string())
}

fn resolve_commit_fingerprint(
    repo: &Repository,
    requested_revision: &str,
) -> anyhow::Result<String> {
    try_resolve_commit_fingerprint_with_alternates(repo, requested_revision).ok_or_else(|| {
        anyhow::anyhow!("failed to resolve git revision '{requested_revision}' to a commit")
    })
}

fn try_resolve_commit_fingerprint_with_alternates(
    repo: &Repository,
    requested_revision: &str,
) -> Option<String> {
    for revision in revision_candidates(requested_revision) {
        if let Some(fingerprint) = try_resolve_commit_fingerprint(repo, &revision) {
            return Some(fingerprint);
        }
    }
    None
}

fn revision_candidates(requested_revision: &str) -> Vec<String> {
    let mut out = vec![requested_revision.to_string()];
    if let Some(alt) = alternate_tag_revision(requested_revision) {
        out.push(alt);
    }
    out
}

fn ensure_checkout_exists(
    mirror_repo_path: &Path,
    checkout_path: &Path,
    source_fingerprint: &str,
) -> anyhow::Result<()> {
    if checkout_path.exists() {
        if checkout_path.is_dir() {
            return Ok(());
        }
        anyhow::bail!(
            "cache checkout path exists and is not a directory: {}",
            checkout_path.display()
        );
    }

    if let Some(parent) = checkout_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create cache checkout parent directory {}",
                parent.display()
            )
        })?;
    }

    let mirror_repo_url = mirror_repo_path.to_string_lossy().to_string();
    let checkout_repo = Repository::clone(&mirror_repo_url, checkout_path).with_context(|| {
        format!(
            "failed to create cache checkout from {} at {}",
            mirror_repo_path.display(),
            checkout_path.display()
        )
    })?;

    let oid = Oid::from_str(source_fingerprint).with_context(|| {
        format!("resolved source fingerprint is not a valid OID: {source_fingerprint}")
    })?;

    let object = checkout_repo
        .find_object(oid, None)
        .with_context(|| format!("failed to find OID {source_fingerprint} in checkout repo"))?;
    checkout_repo
        .checkout_tree(&object, Some(CheckoutBuilder::new().force()))
        .with_context(|| {
            format!(
                "failed to checkout OID {source_fingerprint} into {}",
                checkout_path.display()
            )
        })?;
    checkout_repo
        .set_head_detached(oid)
        .with_context(|| format!("failed to detach HEAD at OID {source_fingerprint}"))?;

    Ok(())
}

fn ensure_symlink(target: &Path, link: &Path) -> anyhow::Result<()> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create project link parent directory {}",
                parent.display()
            )
        })?;
    }

    match fs::symlink_metadata(link) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                let existing_target = fs::read_link(link).with_context(|| {
                    format!("failed to read existing symlink at {}", link.display())
                })?;
                if existing_target == target {
                    return Ok(());
                }
                fs::remove_file(link).with_context(|| {
                    format!("failed to remove existing symlink at {}", link.display())
                })?;
            } else if metadata.is_file() {
                fs::remove_file(link).with_context(|| {
                    format!("failed to remove existing file at {}", link.display())
                })?;
            } else {
                anyhow::bail!(
                    "refusing to replace existing directory at {}; expected a symlink",
                    link.display()
                );
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to inspect existing path at {}", link.display()));
        }
    }

    create_symlink(target, link).with_context(|| {
        format!(
            "failed to create symlink {} -> {}",
            link.display(),
            target.display()
        )
    })?;
    Ok(())
}

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targeted_refspecs_for_tag_like_revision() {
        let refspecs = targeted_refspecs("v18.3.1");
        assert_eq!(
            refspecs,
            vec![
                "refs/tags/v18.3.1:refs/tags/v18.3.1",
                "refs/heads/v18.3.1:refs/heads/v18.3.1",
                "v18.3.1",
                "refs/tags/18.3.1:refs/tags/18.3.1",
                "refs/heads/18.3.1:refs/heads/18.3.1",
                "18.3.1"
            ]
        );
        assert!(!refspecs.iter().any(|r| r.contains("refs/heads/*")));
        assert!(!refspecs.iter().any(|r| r.contains("refs/tags/*")));
    }

    #[test]
    fn targeted_refspecs_for_plain_semver_include_v_prefix_variant() {
        let refspecs = targeted_refspecs("2.32.3");
        assert_eq!(
            refspecs,
            vec![
                "refs/tags/2.32.3:refs/tags/2.32.3",
                "refs/heads/2.32.3:refs/heads/2.32.3",
                "2.32.3",
                "refs/tags/v2.32.3:refs/tags/v2.32.3",
                "refs/heads/v2.32.3:refs/heads/v2.32.3",
                "v2.32.3",
            ]
        );
    }

    #[test]
    fn targeted_refspecs_for_full_ref() {
        let refspecs = targeted_refspecs("refs/heads/main");
        assert_eq!(
            refspecs,
            vec!["refs/heads/main:refs/heads/main", "refs/heads/main"]
        );
    }

    #[test]
    fn targeted_refspecs_for_sha() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let refspecs = targeted_refspecs(sha);
        assert_eq!(
            refspecs,
            vec![
                sha,
                "HEAD:refs/heads/pkgrep-head",
                "refs/heads/main:refs/heads/main",
                "refs/heads/master:refs/heads/master"
            ]
        );
    }

    #[test]
    fn detects_hex_revision() {
        assert!(looks_like_hex_revision("deadbee"));
        assert!(looks_like_hex_revision(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(!looks_like_hex_revision("v18.3.1"));
        assert!(!looks_like_hex_revision("HEAD"));
    }

    #[test]
    fn shallow_fetch_support_detection() {
        assert!(supports_shallow_fetch(
            "https://github.com/facebook/react.git"
        ));
        assert!(supports_shallow_fetch(
            "ssh://git@github.com/facebook/react.git"
        ));
        assert!(!supports_shallow_fetch("/tmp/repo"));
        assert!(!supports_shallow_fetch("./repo"));
        assert!(!supports_shallow_fetch("../repo"));
        assert!(!supports_shallow_fetch("file:///tmp/repo"));
    }
}
