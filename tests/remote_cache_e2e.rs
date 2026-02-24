use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use git2::Repository;
use predicates::prelude::*;
use tempfile::TempDir;
use testcontainers::core::wait::ExitWaitStrategy;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::SyncRunner;
use testcontainers::{GenericImage, ImageExt};

const AZURITE_ACCOUNT_KEY: &str =
    "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";
const SEAWEED_BUCKET: &str = "pkgrep-cache";
const SEAWEED_ACCESS_KEY_ID: &str = "pkgrep";
const SEAWEED_SECRET_ACCESS_KEY: &str = "pkgrepsecret";
const AZURITE_CONTAINER: &str = "pkgrep-cache";
const SEAWEED_S3_PORT: u16 = 8333;
const AZURITE_BLOB_PORT: u16 = 10000;
const SEAWEED_S3_CONFIG_PATH: &str = ".dev/seaweedfs/s3.json";

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

fn init_local_git_repo(path: &Path) -> String {
    std::fs::create_dir_all(path).expect("create local git repo dir");
    let repo = Repository::init(path).expect("init repo");

    std::fs::write(path.join("README.md"), "remote e2e fixture\n").expect("write fixture file");

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

fn unique_prefix(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("e2e/{name}/{nanos}-{}", std::process::id())
}

fn unique_name(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}-{}", std::process::id())
}

fn format_http_endpoint(host: String, port: u16, path_suffix: Option<&str>) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };

    match path_suffix {
        Some(suffix) => format!("http://{host}:{port}/{suffix}"),
        None => format!("http://{host}:{port}"),
    }
}

fn start_seaweedfs(network: &str, container_name: &str) -> testcontainers::Container<GenericImage> {
    let s3_config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(SEAWEED_S3_CONFIG_PATH);
    assert!(
        s3_config_path.exists(),
        "expected SeaweedFS s3 config fixture at {}",
        s3_config_path.display()
    );

    GenericImage::new("chrislusf/seaweedfs", "latest")
        .with_exposed_port(SEAWEED_S3_PORT.tcp())
        .with_wait_for(WaitFor::millis(500))
        .with_network(network)
        .with_container_name(container_name)
        .with_copy_to("/etc/seaweedfs/s3.json", s3_config_path.as_path())
        .with_cmd([
            "server",
            "-s3",
            "-s3.config=/etc/seaweedfs/s3.json",
            "-dir=/data",
        ])
        .start()
        .expect("start SeaweedFS container")
}

fn init_seaweed_bucket(network: &str, seaweed_container_name: &str) {
    let endpoint = format!("http://{seaweed_container_name}:{SEAWEED_S3_PORT}");
    let init_cmd = format!(
        "for _ in $(seq 1 60); do \
            aws s3api head-bucket --bucket {bucket} --endpoint-url {endpoint} >/dev/null 2>&1 \
              || aws s3api create-bucket --bucket {bucket} --endpoint-url {endpoint} --region us-east-1 >/dev/null 2>&1; \
            aws s3api put-object --bucket {bucket} --key pkgrep-warmup --body /etc/hosts --endpoint-url {endpoint} >/dev/null 2>&1 && exit 0; \
            sleep 1; \
        done; \
        exit 1",
        bucket = SEAWEED_BUCKET
    );

    let _ = GenericImage::new("amazon/aws-cli", "2.31.17")
        .with_entrypoint("sh")
        .with_wait_for(WaitFor::exit(ExitWaitStrategy::new().with_exit_code(0)))
        .with_network(network)
        .with_env_var("AWS_ACCESS_KEY_ID", SEAWEED_ACCESS_KEY_ID)
        .with_env_var("AWS_SECRET_ACCESS_KEY", SEAWEED_SECRET_ACCESS_KEY)
        .with_env_var("AWS_REGION", "us-east-1")
        .with_env_var("AWS_DEFAULT_REGION", "us-east-1")
        .with_cmd(["-ceu", &init_cmd])
        .start()
        .expect("initialize SeaweedFS bucket with aws-cli");
}

fn start_azurite(network: &str, container_name: &str) -> testcontainers::Container<GenericImage> {
    GenericImage::new("mcr.microsoft.com/azure-storage/azurite", "latest")
        .with_exposed_port(AZURITE_BLOB_PORT.tcp())
        .with_wait_for(WaitFor::millis(500))
        .with_network(network)
        .with_container_name(container_name)
        .with_cmd([
            "azurite-blob",
            "--blobHost",
            "0.0.0.0",
            "--blobPort",
            "10000",
        ])
        .start()
        .expect("start Azurite container")
}

fn init_azurite_container(network: &str, azurite_container_name: &str) {
    let connection_string = format!(
        "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey={};BlobEndpoint=http://{}:{}/devstoreaccount1;",
        AZURITE_ACCOUNT_KEY, azurite_container_name, AZURITE_BLOB_PORT
    );
    let init_cmd = format!(
        "for _ in $(seq 1 60); do \
            az storage container create --name {container} --connection-string \"$PKGREP_AZURE_CONNECTION_STRING\" --only-show-errors --output none >/dev/null 2>&1 && exit 0; \
            sleep 1; \
        done; \
        exit 1",
        container = AZURITE_CONTAINER
    );

    let _ = GenericImage::new("mcr.microsoft.com/azure-cli", "2.73.0")
        .with_entrypoint("sh")
        .with_wait_for(WaitFor::exit(ExitWaitStrategy::new().with_exit_code(0)))
        .with_network(network)
        .with_env_var("PKGREP_AZURE_CONNECTION_STRING", connection_string)
        .with_cmd(["-ceu", &init_cmd])
        .start()
        .expect("initialize Azurite container with azure-cli");
}

#[test]
#[ignore = "requires docker (testcontainers)"]
fn e2e_s3_remote_roundtrip() {
    let network = unique_name("pkgrep-e2e-s3-net");
    let seaweed_name = unique_name("pkgrep-e2e-s3");
    let seaweed = start_seaweedfs(&network, &seaweed_name);
    init_seaweed_bucket(&network, &seaweed_name);

    let endpoint = format_http_endpoint(
        "127.0.0.1".to_string(),
        seaweed
            .get_host_port_ipv4(SEAWEED_S3_PORT.tcp())
            .expect("SeaweedFS mapped port"),
        None,
    );

    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);
    let prefix = unique_prefix("s3");

    cmd_in_temp(&temp)
        .env("PKGREP_BACKEND", "s3")
        .env("PKGREP_OBJECT_STORE_ENDPOINT", &endpoint)
        .env("PKGREP_OBJECT_STORE_BUCKET", SEAWEED_BUCKET)
        .env("PKGREP_OBJECT_STORE_ACCESS_KEY_ID", SEAWEED_ACCESS_KEY_ID)
        .env(
            "PKGREP_OBJECT_STORE_SECRET_ACCESS_KEY",
            SEAWEED_SECRET_ACCESS_KEY,
        )
        .env("PKGREP_OBJECT_STORE_PREFIX", &prefix)
        .env("PKGREP_OBJECT_STORE_REGION", "us-east-1")
        .args(["pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull completed: total=1"))
        .stdout(predicate::str::contains("fetched_from_git=1"))
        .stdout(predicate::str::contains("published_to_remote=1"));

    std::fs::remove_dir_all(temp.path().join(".pkgrep")).expect("remove project links");
    std::fs::remove_dir_all(temp.path().join("cache")).expect("remove cache");

    cmd_in_temp(&temp)
        .env("PKGREP_BACKEND", "s3")
        .env("PKGREP_OBJECT_STORE_ENDPOINT", &endpoint)
        .env("PKGREP_OBJECT_STORE_BUCKET", SEAWEED_BUCKET)
        .env("PKGREP_OBJECT_STORE_ACCESS_KEY_ID", SEAWEED_ACCESS_KEY_ID)
        .env(
            "PKGREP_OBJECT_STORE_SECRET_ACCESS_KEY",
            SEAWEED_SECRET_ACCESS_KEY,
        )
        .env("PKGREP_OBJECT_STORE_PREFIX", &prefix)
        .env("PKGREP_OBJECT_STORE_REGION", "us-east-1")
        .args(["cache", "hydrate", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Hydrate completed: total=1 hydrated=1",
        ));

    let link = first_symlink_entry(&temp.path().join(".pkgrep").join("deps").join("git"));
    let metadata = std::fs::symlink_metadata(&link).expect("link metadata");
    assert!(metadata.file_type().is_symlink());
    let target = std::fs::read_link(&link).expect("read link");
    assert!(target.join("README.md").exists());
}

#[test]
#[ignore = "requires docker (testcontainers)"]
fn e2e_azure_remote_roundtrip() {
    let network = unique_name("pkgrep-e2e-az-net");
    let azurite_name = unique_name("pkgrep-e2e-az");
    let azurite = start_azurite(&network, &azurite_name);
    init_azurite_container(&network, &azurite_name);

    let endpoint = format_http_endpoint(
        "127.0.0.1".to_string(),
        azurite
            .get_host_port_ipv4(AZURITE_BLOB_PORT.tcp())
            .expect("Azurite mapped port"),
        Some("devstoreaccount1"),
    );

    let temp = TempDir::new().expect("tempdir");
    let repo_path = temp.path().join("source-repo");
    let revision = init_local_git_repo(&repo_path);
    let dep_spec = format!("git:{}@{}", repo_path.display(), revision);
    let prefix = unique_prefix("azure");

    cmd_in_temp(&temp)
        .env("PKGREP_BACKEND", "azure_blob")
        .env("PKGREP_OBJECT_STORE_ENDPOINT", &endpoint)
        .env("PKGREP_OBJECT_STORE_BUCKET", AZURITE_CONTAINER)
        .env("PKGREP_AZURE_ACCOUNT_NAME", "devstoreaccount1")
        .env("PKGREP_AZURE_ACCOUNT_KEY", AZURITE_ACCOUNT_KEY)
        .env("PKGREP_OBJECT_STORE_PREFIX", &prefix)
        .args(["pull", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pull completed: total=1"))
        .stdout(predicate::str::contains("fetched_from_git=1"));

    std::fs::remove_dir_all(temp.path().join(".pkgrep")).expect("remove project links");
    std::fs::remove_dir_all(temp.path().join("cache")).expect("remove cache");

    cmd_in_temp(&temp)
        .env("PKGREP_BACKEND", "azure_blob")
        .env("PKGREP_OBJECT_STORE_ENDPOINT", &endpoint)
        .env("PKGREP_OBJECT_STORE_BUCKET", AZURITE_CONTAINER)
        .env("PKGREP_AZURE_ACCOUNT_NAME", "devstoreaccount1")
        .env("PKGREP_AZURE_ACCOUNT_KEY", AZURITE_ACCOUNT_KEY)
        .env("PKGREP_OBJECT_STORE_PREFIX", &prefix)
        .args(["cache", "hydrate", &dep_spec])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Hydrate completed: total=1 hydrated=1",
        ));

    let link = first_symlink_entry(&temp.path().join(".pkgrep").join("deps").join("git"));
    let metadata = std::fs::symlink_metadata(&link).expect("link metadata");
    assert!(metadata.file_type().is_symlink());
    let target = std::fs::read_link(&link).expect("read link");
    assert!(target.join("README.md").exists());
}
