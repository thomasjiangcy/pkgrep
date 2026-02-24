use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use git2::Repository;
use predicates::prelude::*;
use serde_json::Value;
use serde_json::json;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn cmd_in_temp(temp: &TempDir) -> Command {
    let mut cmd = cargo_bin_cmd!("pkgrep");
    let xdg_config = temp.path().join("xdg_config");
    let cache_dir = temp.path().join("cache");
    std::fs::create_dir_all(&xdg_config).expect("create config dir");
    std::fs::create_dir_all(&cache_dir).expect("create cache dir");

    cmd.current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("PKGREP_CACHE_DIR", &cache_dir);

    cmd
}

fn configured_cache_dir(temp: &TempDir) -> PathBuf {
    temp.path().join("cache")
}

fn read_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).expect("read json file");
    serde_json::from_slice(&bytes).expect("parse json")
}

fn count_cached_checkouts(cache_dir: &Path) -> usize {
    let root = cache_dir.join("sources");
    let mut count = 0usize;
    collect_checkout_dirs(&root, &mut count);
    count
}

fn collect_checkout_dirs(path: &Path, count: &mut usize) {
    if !path.exists() {
        return;
    }
    let metadata = std::fs::symlink_metadata(path).expect("stat path");
    if !metadata.is_dir() {
        return;
    }
    if path.join(".git").exists() {
        *count += 1;
        return;
    }
    for entry in std::fs::read_dir(path).expect("read dir") {
        let entry = entry.expect("entry");
        collect_checkout_dirs(&entry.path(), count);
    }
}

fn count_cached_mirrors(cache_dir: &Path) -> usize {
    let repos_root = cache_dir.join("repos");
    if !repos_root.exists() {
        return 0;
    }

    let mut count = 0usize;
    for ecosystem_entry in std::fs::read_dir(&repos_root).expect("read repos dir") {
        let ecosystem_entry = ecosystem_entry.expect("ecosystem entry");
        let ecosystem_path = ecosystem_entry.path();
        if !std::fs::symlink_metadata(&ecosystem_path)
            .expect("ecosystem metadata")
            .is_dir()
        {
            continue;
        }

        for mirror_entry in std::fs::read_dir(ecosystem_path).expect("read ecosystem dir") {
            let mirror_entry = mirror_entry.expect("mirror entry");
            let mirror_path = mirror_entry.path();
            let file_name = mirror_entry.file_name().to_string_lossy().to_string();
            if file_name.ends_with(".git")
                && std::fs::symlink_metadata(&mirror_path)
                    .expect("mirror metadata")
                    .is_dir()
            {
                count += 1;
            }
        }
    }
    count
}

fn init_local_git_repo(path: &Path) -> String {
    std::fs::create_dir_all(path).expect("create local git repo dir");
    let repo = Repository::init(path).expect("init repo");

    std::fs::write(path.join("README.md"), "fixture repo\n").expect("write fixture file");

    let mut index = repo.index().expect("index");
    index
        .add_path(Path::new("README.md"))
        .expect("add path to index");
    index.write().expect("write index");

    let tree_id = index.write_tree().expect("write tree");
    let tree = repo.find_tree(tree_id).expect("find tree");
    let sig = git2::Signature::now("pkgrep-test", "pkgrep-test@example.com").expect("signature");

    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
        .expect("commit");

    oid.to_string()
}

fn first_symlink_entry(path: &Path) -> PathBuf {
    let mut entries = Vec::new();
    collect_symlink_entries(path, &mut entries);
    entries.sort();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one symlink entry under {}",
        path.display()
    );
    entries.remove(0)
}

fn collect_symlink_entries(path: &Path, out: &mut Vec<PathBuf>) {
    let metadata = std::fs::symlink_metadata(path).expect("stat path");
    if metadata.file_type().is_symlink() {
        out.push(path.to_path_buf());
        return;
    }

    if !metadata.is_dir() {
        return;
    }

    for entry in std::fs::read_dir(path).expect("read dir") {
        let entry = entry.expect("entry");
        collect_symlink_entries(&entry.path(), out);
    }
}

#[test]
fn remove_without_yes_is_noop() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["remove", "npm:react@18.3.1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No-op: pass --yes to remove linked dependencies",
        ));
}

#[test]
fn skill_install_defaults_to_project_agents_skills() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["skill", "install"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed skill: "));

    let installed_skill = temp
        .path()
        .join(".agents")
        .join("skills")
        .join("pkgrep-usage")
        .join("SKILL.md");
    assert!(
        installed_skill.exists(),
        "expected installed skill file at {}",
        installed_skill.display()
    );
}

#[test]
fn skill_install_global_uses_home_agents_skills_and_force_replaces() {
    let temp = TempDir::new().expect("tempdir");
    let fake_home = temp.path().join("home");
    std::fs::create_dir_all(&fake_home).expect("create fake home");

    cmd_in_temp(&temp)
        .env("HOME", &fake_home)
        .args(["skill", "install", "--mode", "global"])
        .assert()
        .success();

    let global_skill_dir = fake_home
        .join(".agents")
        .join("skills")
        .join("pkgrep-usage");
    let marker_path = global_skill_dir.join("MARKER.txt");
    std::fs::write(&marker_path, "marker").expect("write marker");

    cmd_in_temp(&temp)
        .env("HOME", &fake_home)
        .args(["skill", "install", "--mode", "global"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("skill destination already exists"));

    cmd_in_temp(&temp)
        .env("HOME", &fake_home)
        .args(["skill", "install", "--mode", "global", "--force"])
        .assert()
        .success();

    assert!(
        !marker_path.exists(),
        "expected force install to replace existing skill directory"
    );
    assert!(
        global_skill_dir.join("SKILL.md").exists(),
        "expected SKILL.md in {}",
        global_skill_dir.display()
    );
}

#[test]
fn cache_clean_without_yes_is_noop() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["cache", "clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No-op: pass --yes to clean local cache",
        ));
}

#[test]
fn hydrate_requires_remote_backend() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .env("PKGREP_BACKEND", "local")
        .args(["cache", "hydrate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("hydrate_requires_remote_backend"));
}

#[test]
fn verbose_flag_is_accepted() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["--verbose", "cache", "clean"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No-op: pass --yes to clean local cache",
        ));
}

#[test]
fn verbose_pull_uses_targeted_refspecs_without_wildcards() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["--verbose", "pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "fetching targeted revision from origin",
        ))
        .stdout(predicate::str::contains("refs/heads/*").not())
        .stdout(predicate::str::contains("refs/tags/*").not());
}

#[test]
fn second_verbose_pull_uses_local_mirror_without_refetch() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["--verbose", "pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "fetching targeted revision from origin",
        ));

    cmd_in_temp(&temp)
        .args(["--verbose", "pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "requested revision already present in mirror repo; skipping fetch",
        ))
        .stdout(predicate::str::contains("fetching targeted revision from origin").not());
}

#[test]
fn invalid_dep_spec_fails_fast() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["pull", "npm:"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid dep spec"));
}

#[test]
fn pull_shorthand_fails_without_supported_lockfiles() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["pull", "zod"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cannot infer shorthand dependency ecosystem",
        ))
        .stderr(predicate::str::contains("no supported lockfiles detected"));
}

#[test]
fn pull_shorthand_fails_with_multiple_supported_lockfile_ecosystems() {
    let temp = TempDir::new().expect("tempdir");
    std::fs::write(temp.path().join("package-lock.json"), "{}").expect("write package-lock");
    std::fs::write(temp.path().join("uv.lock"), "").expect("write uv.lock");

    cmd_in_temp(&temp)
        .args(["pull", "zod"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "multiple supported lockfile ecosystems detected",
        ));
}

#[test]
fn pull_shorthand_infers_npm_with_single_js_lockfile() {
    let temp = TempDir::new().expect("tempdir");
    std::fs::write(temp.path().join("package-lock.json"), "{}").expect("write package-lock");

    cmd_in_temp(&temp)
        .env("PKGREP_NPM_REGISTRY_URL", "not-a-url")
        .args(["pull", "zod@3.23.8"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "inferred shorthand 'zod@3.23.8' as 'npm:zod@3.23.8'",
        ))
        .stdout(predicate::str::contains(
            "resolving package metadata for npm:zod@3.23.8",
        ))
        .stderr(predicate::str::contains("invalid npm registry URL"));
}

#[test]
fn pull_shorthand_infers_pypi_with_single_python_lockfile() {
    let temp = TempDir::new().expect("tempdir");
    std::fs::write(temp.path().join("uv.lock"), "").expect("write uv.lock");

    cmd_in_temp(&temp)
        .env("PKGREP_PYPI_REGISTRY_URL", "not-a-url")
        .args(["pull", "requests@2.32.3"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "inferred shorthand 'requests@2.32.3' as 'pypi:requests@2.32.3'",
        ))
        .stdout(predicate::str::contains(
            "resolving package metadata for pypi:requests@2.32.3",
        ))
        .stderr(predicate::str::contains("invalid pypi registry URL"));
}

#[test]
fn pull_without_specs_in_empty_folder_is_noop() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["pull"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No-op: no dep specs provided and no supported project lockfiles found",
        ));
}

#[test]
fn pull_without_specs_with_non_git_lockfile_entries_is_noop() {
    let temp = TempDir::new().expect("tempdir");
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("js")
        .join("package-lock.json");
    let lock_path = temp.path().join("package-lock.json");
    std::fs::copy(&fixture, &lock_path).expect("copy package-lock fixture");

    cmd_in_temp(&temp)
        .args(["pull"])
        .assert()
        .success()
        .stdout(predicate::str::contains("none had git source hints"));
}

#[test]
fn pull_with_explicit_git_spec_materializes_and_links() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);

    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);
    cmd_in_temp(&temp)
        .args(["pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull completed: total=1"));

    let link = first_symlink_entry(&temp.path().join(".pkgrep").join("deps").join("git"));
    let metadata = std::fs::symlink_metadata(&link).expect("link metadata");
    assert!(metadata.file_type().is_symlink());
    let link_display = link.to_string_lossy();
    assert!(link_display.contains("source-repo@"));
    assert!(!link_display.contains("b64_"));

    let target = std::fs::read_link(&link).expect("read link");
    assert!(
        target.exists(),
        "symlink target does not exist: {}",
        target.display()
    );
    assert!(target.join("README.md").exists());
}

#[test]
fn path_returns_link_when_present() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["pull", &dep_spec])
        .assert()
        .success();

    let link = first_symlink_entry(&temp.path().join(".pkgrep").join("deps").join("git"));
    let link_display = link.display().to_string();

    cmd_in_temp(&temp)
        .args(["path", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains(&link_display));
}

#[test]
fn path_fails_when_dep_is_not_linked() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .args(["path", "git:https://github.com/facebook/react.git@v18.3.1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "dependency is not linked in this project",
        ));
}

#[test]
fn pull_and_remove_update_project_and_global_indexes() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull completed: total=1"));

    let manifest_path = temp.path().join(".pkgrep").join("manifest.json");
    let manifest = read_json(&manifest_path);
    let manifest_entry = manifest
        .get("entries")
        .and_then(|entries| entries.get(&dep_spec))
        .expect("manifest dep entry");
    let link_path = manifest_entry
        .get("link_path")
        .and_then(Value::as_str)
        .expect("manifest link_path");
    let cache_key = manifest_entry
        .get("cache_key")
        .and_then(Value::as_str)
        .expect("manifest cache_key")
        .to_string();
    assert!(
        temp.path().join(link_path).exists(),
        "manifest link path missing"
    );

    let global_index_path = configured_cache_dir(&temp)
        .join("index")
        .join("project_refs.json");
    let global_index = read_json(&global_index_path);
    let global_entry = global_index
        .get("entries")
        .and_then(|entries| entries.get(&cache_key))
        .expect("global index cache entry");
    assert_eq!(
        global_entry
            .get("dep_spec")
            .and_then(Value::as_str)
            .expect("dep_spec"),
        dep_spec
    );
    let canonical_project = temp
        .path()
        .canonicalize()
        .expect("canonical project path")
        .display()
        .to_string();
    let projects = global_entry
        .get("projects")
        .and_then(Value::as_array)
        .expect("projects array");
    assert!(
        projects
            .iter()
            .any(|project| project.as_str() == Some(canonical_project.as_str())),
        "global index missing project reference"
    );

    cmd_in_temp(&temp)
        .args(["remove", &dep_spec, "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Remove completed: removed=1"));

    let manifest_after_remove = read_json(&manifest_path);
    let has_manifest_entry = manifest_after_remove
        .get("entries")
        .and_then(|entries| entries.get(&dep_spec))
        .is_some();
    assert!(!has_manifest_entry, "manifest entry was not removed");

    let global_index_after_remove = read_json(&global_index_path);
    let has_global_entry = global_index_after_remove
        .get("entries")
        .and_then(|entries| entries.get(&cache_key))
        .is_some();
    assert!(!has_global_entry, "global index entry was not removed");
}

#[test]
fn pull_without_specs_uses_package_lock_git_hint() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);

    let package_lock = json!({
        "name": "fixture-js-npm",
        "version": "1.0.0",
        "lockfileVersion": 3,
        "packages": {
            "": {
                "name": "fixture-js-npm",
                "version": "1.0.0",
                "dependencies": {
                    "demo-git-package": "1.0.0"
                }
            },
            "node_modules/demo-git-package": {
                "version": "1.0.0",
                "resolved": format!("git+{}#{}", repo_path.display(), revision),
            }
        }
    });
    std::fs::write(
        temp.path().join("package-lock.json"),
        serde_json::to_vec_pretty(&package_lock).expect("serialize lock"),
    )
    .expect("write package-lock");

    cmd_in_temp(&temp)
        .args(["pull"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull completed: total=1"));

    let npm_links = temp.path().join(".pkgrep").join("deps").join("npm");
    let link = first_symlink_entry(&npm_links);
    let metadata = std::fs::symlink_metadata(&link).expect("link metadata");
    assert!(metadata.file_type().is_symlink());
}

#[test]
fn remove_with_yes_deletes_project_symlink() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["pull", &dep_spec])
        .assert()
        .success();

    let links_root = temp.path().join(".pkgrep").join("deps").join("git");
    let link = first_symlink_entry(&links_root);
    assert!(link.exists());

    cmd_in_temp(&temp)
        .args(["remove", &dep_spec, "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Remove completed: removed=1"));

    assert!(
        !link.exists(),
        "expected link to be removed, found {}",
        link.display()
    );
}

#[test]
fn cache_clean_with_yes_removes_cache_dir() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["pull", &dep_spec])
        .assert()
        .success();

    let cache_dir = configured_cache_dir(&temp);
    assert!(
        cache_dir.exists(),
        "expected cache dir to exist before clean"
    );

    cmd_in_temp(&temp)
        .args(["cache", "clean", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleaned local cache"));

    assert!(
        !cache_dir.exists(),
        "expected cache dir to be removed, found {}",
        cache_dir.display()
    );
}

#[test]
fn cache_prune_requires_yes_and_prunes_stale_checkouts_and_mirrors() {
    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let repo_display = repo_path.display().to_string();
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);

    cmd_in_temp(&temp)
        .args(["pull", &dep_spec])
        .assert()
        .success();
    cmd_in_temp(&temp)
        .args(["remove", &dep_spec, "--yes"])
        .assert()
        .success();

    let cache_dir = configured_cache_dir(&temp);
    assert_eq!(count_cached_checkouts(&cache_dir), 1);
    assert_eq!(count_cached_mirrors(&cache_dir), 1);

    cmd_in_temp(&temp)
        .args(["cache", "prune"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&repo_display))
        .stdout(predicate::str::contains(
            "No-op: pass --yes to prune local cache entries",
        ));

    assert_eq!(count_cached_checkouts(&cache_dir), 1);
    assert_eq!(count_cached_mirrors(&cache_dir), 1);

    cmd_in_temp(&temp)
        .args(["cache", "prune", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Prune completed: removed_checkouts=1",
        ))
        .stdout(predicate::str::contains("removed_mirrors=1"));

    assert_eq!(count_cached_checkouts(&cache_dir), 0);
    assert_eq!(count_cached_mirrors(&cache_dir), 0);
}

#[test]
fn hydrate_remote_backend_requires_bucket_config() {
    let temp = TempDir::new().expect("tempdir");
    cmd_in_temp(&temp)
        .env("PKGREP_BACKEND", "s3")
        .args([
            "cache",
            "hydrate",
            "git:https://example.com/org/repo.git@deadbeef",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("object_store.bucket must be set"));
}
