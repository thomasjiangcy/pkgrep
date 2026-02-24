use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::source::{GitPullTarget, MaterializedSource};

const PROJECT_MANIFEST_SCHEMA_VERSION: u8 = 1;
const GLOBAL_REF_INDEX_SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MirrorRef {
    pub ecosystem: String,
    pub normalized_locator: String,
}

#[derive(Clone, Debug, Default)]
pub struct ReconcileGlobalIndexResult {
    pub stale_project_references_removed: usize,
    pub empty_entries_removed: usize,
    pub index_updated: bool,
    pub live_cache_keys: BTreeSet<String>,
    pub live_mirror_refs: BTreeSet<MirrorRef>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ProjectManifest {
    schema_version: u8,
    entries: BTreeMap<String, ProjectManifestEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ProjectManifestEntry {
    link_path: String,
    cache_key: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct GlobalRefIndex {
    schema_version: u8,
    entries: BTreeMap<String, GlobalRefEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GlobalRefEntry {
    dep_spec: String,
    checkout_path: String,
    projects: BTreeSet<String>,
}

pub fn project_manifest_path(cwd: &Path) -> PathBuf {
    cwd.join(".pkgrep").join("manifest.json")
}

pub fn global_ref_index_path(cache_root: &Path) -> PathBuf {
    cache_root.join("index").join("project_refs.json")
}

pub fn reconcile_global_index(cache_root: &Path) -> anyhow::Result<ReconcileGlobalIndexResult> {
    let path = global_ref_index_path(cache_root);
    let mut index: GlobalRefIndex = read_json_or_default(&path)?;
    ensure_global_ref_index_defaults(&mut index);

    let mut cached_project_cache_keys: BTreeMap<String, Option<BTreeSet<String>>> = BTreeMap::new();
    let mut stale_project_references_removed = 0usize;
    let mut index_updated = false;

    for (cache_key, entry) in &mut index.entries {
        let mut kept_projects = BTreeSet::new();
        for project_root in &entry.projects {
            if project_references_cache_key(project_root, cache_key, &mut cached_project_cache_keys)
            {
                kept_projects.insert(project_root.clone());
            }
        }

        stale_project_references_removed +=
            entry.projects.len().saturating_sub(kept_projects.len());
        if kept_projects != entry.projects {
            entry.projects = kept_projects;
            index_updated = true;
        }
    }

    let before_entries = index.entries.len();
    index.entries.retain(|_, entry| !entry.projects.is_empty());
    let empty_entries_removed = before_entries.saturating_sub(index.entries.len());
    if empty_entries_removed > 0 {
        index_updated = true;
    }

    if index_updated {
        write_json_atomic(&path, &index)?;
    }

    let live_cache_keys = index.entries.keys().cloned().collect::<BTreeSet<_>>();
    let live_mirror_refs = index
        .entries
        .keys()
        .filter_map(|cache_key| mirror_ref_from_cache_key(cache_key))
        .collect::<BTreeSet<_>>();

    Ok(ReconcileGlobalIndexResult {
        stale_project_references_removed,
        empty_entries_removed,
        index_updated,
        live_cache_keys,
        live_mirror_refs,
    })
}

pub fn record_link(
    cwd: &Path,
    cache_root: &Path,
    target: &GitPullTarget,
    materialized: &MaterializedSource,
) -> anyhow::Result<()> {
    let dep_spec = dep_spec(target);
    let project_root = normalize_project_root(cwd);
    let link_path = path_for_manifest(cwd, &materialized.project_link_path);

    update_project_manifest(cwd, |manifest| {
        manifest.entries.insert(
            dep_spec.clone(),
            ProjectManifestEntry {
                link_path,
                cache_key: materialized.cache_key.clone(),
            },
        );
    })?;

    update_global_ref_index(cache_root, |index| {
        let entry = index
            .entries
            .entry(materialized.cache_key.clone())
            .or_insert_with(|| GlobalRefEntry {
                dep_spec: dep_spec.clone(),
                checkout_path: materialized.checkout_path.display().to_string(),
                projects: BTreeSet::new(),
            });
        entry.dep_spec = dep_spec;
        entry.checkout_path = materialized.checkout_path.display().to_string();
        entry.projects.insert(project_root);
    })?;

    Ok(())
}

pub fn record_unlink(
    cwd: &Path,
    cache_root: &Path,
    removed_link_path: &Path,
    symlink_target: Option<&Path>,
) -> anyhow::Result<()> {
    let removed_link = path_for_manifest(cwd, removed_link_path);
    update_project_manifest(cwd, |manifest| {
        manifest
            .entries
            .retain(|_, entry| entry.link_path != removed_link);
    })?;

    let Some(symlink_target) = symlink_target else {
        return Ok(());
    };
    let Some(cache_key) = cache_key_from_checkout_path(cache_root, symlink_target) else {
        return Ok(());
    };
    let project_root = normalize_project_root(cwd);

    update_global_ref_index(cache_root, |index| {
        if let Some(entry) = index.entries.get_mut(&cache_key) {
            entry.projects.remove(&project_root);
            if entry.projects.is_empty() {
                index.entries.remove(&cache_key);
            }
        }
    })?;

    Ok(())
}

fn update_project_manifest(
    cwd: &Path,
    mutator: impl FnOnce(&mut ProjectManifest),
) -> anyhow::Result<()> {
    let path = project_manifest_path(cwd);
    let mut manifest: ProjectManifest = read_json_or_default(&path)?;
    ensure_project_manifest_defaults(&mut manifest);
    mutator(&mut manifest);
    write_json_atomic(&path, &manifest)?;
    Ok(())
}

fn update_global_ref_index(
    cache_root: &Path,
    mutator: impl FnOnce(&mut GlobalRefIndex),
) -> anyhow::Result<()> {
    let path = global_ref_index_path(cache_root);
    let mut index: GlobalRefIndex = read_json_or_default(&path)?;
    ensure_global_ref_index_defaults(&mut index);
    mutator(&mut index);
    write_json_atomic(&path, &index)?;
    Ok(())
}

fn read_json_or_default<T>(path: &Path) -> anyhow::Result<T>
where
    T: Default + DeserializeOwned,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let bytes =
        fs::read(path).with_context(|| format!("failed to read JSON file {}", path.display()))?;
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse JSON file {}", path.display()))?;
    Ok(value)
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    }

    let payload = serde_json::to_vec_pretty(value).context("failed to serialize JSON payload")?;
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, payload).with_context(|| {
        format!(
            "failed to write temporary JSON file {}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path)
        .with_context(|| format!("failed to atomically replace JSON file {}", path.display()))?;
    Ok(())
}

fn dep_spec(target: &GitPullTarget) -> String {
    format!("git:{}@{}", target.git_url, target.requested_revision)
}

fn normalize_project_root(cwd: &Path) -> String {
    cwd.canonicalize()
        .unwrap_or_else(|_| cwd.to_path_buf())
        .display()
        .to_string()
}

fn path_for_manifest(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd).unwrap_or(path).display().to_string()
}

fn cache_key_from_checkout_path(cache_root: &Path, checkout_path: &Path) -> Option<String> {
    let sources_root = cache_root.join("sources");
    let relative = checkout_path.strip_prefix(sources_root).ok()?;

    let mut components = Vec::new();
    for component in relative.components() {
        components.push(component.as_os_str().to_string_lossy().to_string());
    }

    if components.is_empty() {
        return None;
    }

    Some(components.join("/"))
}

fn mirror_ref_from_cache_key(cache_key: &str) -> Option<MirrorRef> {
    let mut parts = cache_key.split('/');
    let ecosystem = parts.next()?.trim();
    let normalized_locator = parts.next()?.trim();
    if ecosystem.is_empty() || normalized_locator.is_empty() {
        return None;
    }

    Some(MirrorRef {
        ecosystem: ecosystem.to_string(),
        normalized_locator: normalized_locator.to_string(),
    })
}

fn project_references_cache_key(
    project_root: &str,
    cache_key: &str,
    cached_project_cache_keys: &mut BTreeMap<String, Option<BTreeSet<String>>>,
) -> bool {
    let cache_keys = cached_project_cache_keys
        .entry(project_root.to_string())
        .or_insert_with(|| load_project_cache_keys(Path::new(project_root)));

    cache_keys
        .as_ref()
        .is_some_and(|project_cache_keys| project_cache_keys.contains(cache_key))
}

fn load_project_cache_keys(project_root: &Path) -> Option<BTreeSet<String>> {
    let path = project_manifest_path(project_root);
    if !path.exists() {
        return None;
    }

    let bytes = fs::read(&path).ok()?;
    let manifest = serde_json::from_slice::<ProjectManifest>(&bytes).ok()?;

    Some(
        manifest
            .entries
            .into_values()
            .map(|entry| entry.cache_key)
            .collect(),
    )
}

fn ensure_project_manifest_defaults(manifest: &mut ProjectManifest) {
    if manifest.schema_version == 0 {
        manifest.schema_version = PROJECT_MANIFEST_SCHEMA_VERSION;
    }
}

fn ensure_global_ref_index_defaults(index: &mut GlobalRefIndex) {
    if index.schema_version == 0 {
        index.schema_version = GLOBAL_REF_INDEX_SCHEMA_VERSION;
    }
}
