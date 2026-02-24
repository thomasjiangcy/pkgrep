mod object_store;

use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::Context;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use opendal::{ErrorKind, Operator};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::depspec;
use crate::source::{self, GitPullTarget, MaterializedSource};

const METADATA_FILE: &str = "metadata.json";
const SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Debug)]
pub enum HydrateOutcome {
    Hydrated(MaterializedSource),
    AlreadyPresent(MaterializedSource),
    NotFound,
}

pub struct RemoteCacheClient {
    operator: Operator,
    runtime: tokio::runtime::Runtime,
    prefix: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RemoteSourceMetadata {
    schema_version: u8,
    ecosystem: String,
    locator: String,
    git_url: String,
    requested_revision: String,
    source_fingerprint: String,
    archive_object_key: String,
}

impl RemoteCacheClient {
    pub fn from_config(config: &Config) -> anyhow::Result<Option<Self>> {
        let Some(operator) = object_store::operator_from_config(config)? else {
            return Ok(None);
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to build runtime for remote cache operations")?;

        Ok(Some(Self {
            operator,
            runtime,
            prefix: normalize_prefix(config.object_store.prefix.as_deref().unwrap_or("")),
        }))
    }

    pub fn hydrate_git_source(
        &self,
        cwd: &Path,
        config: &Config,
        target: &GitPullTarget,
    ) -> anyhow::Result<HydrateOutcome> {
        let metadata_key = self.metadata_key(target);
        let metadata_bytes = match self.read_object(&metadata_key) {
            Ok(bytes) => bytes,
            Err(err) if is_not_found(&err) => return Ok(HydrateOutcome::NotFound),
            Err(err) => {
                return Err(err).with_context(|| format!("failed to read {}", metadata_key));
            }
        };

        let metadata: RemoteSourceMetadata = serde_json::from_slice(&metadata_bytes)
            .with_context(|| format!("failed to parse {}", metadata_key))?;
        validate_metadata(target, &metadata)
            .with_context(|| format!("invalid metadata at {}", metadata_key))?;

        let cache_root = source::cache_root_for(cwd, &config.cache_dir);
        let cache_key = depspec::cache_key(
            &target.ecosystem,
            &target.locator,
            &target.requested_revision,
            &metadata.source_fingerprint,
        );
        let checkout_path = cache_root.join("sources").join(&cache_key);

        if checkout_path.exists() {
            let project_link_path = source::link_checkout(cwd, target, &checkout_path)?;
            return Ok(HydrateOutcome::AlreadyPresent(MaterializedSource {
                cache_key,
                source_fingerprint: metadata.source_fingerprint,
                checkout_path,
                project_link_path,
                git_fetch_performed: false,
            }));
        }

        let archive_bytes = self
            .read_object(&metadata.archive_object_key)
            .with_context(|| {
                format!(
                    "failed to read archive object {}",
                    metadata.archive_object_key
                )
            })?;
        unpack_archive_into_dir(&archive_bytes, &checkout_path)?;
        let project_link_path = source::link_checkout(cwd, target, &checkout_path)?;

        Ok(HydrateOutcome::Hydrated(MaterializedSource {
            cache_key,
            source_fingerprint: metadata.source_fingerprint,
            checkout_path,
            project_link_path,
            git_fetch_performed: false,
        }))
    }

    pub fn publish_git_source(
        &self,
        target: &GitPullTarget,
        materialized: &MaterializedSource,
    ) -> anyhow::Result<()> {
        let archive_key = self.archive_key(target, &materialized.source_fingerprint);
        if !self.exists(&archive_key)? {
            let archive = archive_directory(&materialized.checkout_path)?;
            self.write_object(&archive_key, archive)
                .with_context(|| format!("failed to write {}", archive_key))?;
        }

        let metadata = RemoteSourceMetadata {
            schema_version: SCHEMA_VERSION,
            ecosystem: target.ecosystem.as_str().to_string(),
            locator: target.locator.clone(),
            git_url: target.git_url.clone(),
            requested_revision: target.requested_revision.clone(),
            source_fingerprint: materialized.source_fingerprint.clone(),
            archive_object_key: archive_key,
        };
        let metadata_key = self.metadata_key(target);
        let payload = serde_json::to_vec_pretty(&metadata)
            .context("failed to serialize remote source metadata")?;
        self.write_object(&metadata_key, payload)
            .with_context(|| format!("failed to write {}", metadata_key))?;

        Ok(())
    }

    fn metadata_key(&self, target: &GitPullTarget) -> String {
        format!("{}/{}", self.target_prefix(target), METADATA_FILE)
    }

    fn archive_key(&self, target: &GitPullTarget, source_fingerprint: &str) -> String {
        format!(
            "{}/{}.tar.gz",
            self.target_prefix(target),
            source_fingerprint
        )
    }

    fn target_prefix(&self, target: &GitPullTarget) -> String {
        let locator = depspec::normalize_locator(&target.locator);
        let relative = format!(
            "sources/{}/{}/{}",
            target.ecosystem.as_str(),
            locator,
            target.requested_revision
        );
        with_prefix(&self.prefix, &relative)
    }

    fn exists(&self, key: &str) -> anyhow::Result<bool> {
        self.runtime
            .block_on(self.operator.exists(key))
            .with_context(|| format!("failed to check existence of {}", key))
    }

    fn read_object(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let buffer = self.runtime.block_on(self.operator.read(key))?;
        Ok(buffer.to_vec())
    }

    fn write_object(&self, key: &str, payload: Vec<u8>) -> anyhow::Result<()> {
        self.runtime
            .block_on(self.operator.write(key, payload))
            .with_context(|| format!("failed to write object {}", key))?;
        Ok(())
    }
}

fn validate_metadata(
    target: &GitPullTarget,
    metadata: &RemoteSourceMetadata,
) -> anyhow::Result<()> {
    if metadata.schema_version != SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported metadata schema version {} (expected {})",
            metadata.schema_version,
            SCHEMA_VERSION
        );
    }
    if metadata.ecosystem != target.ecosystem.as_str() {
        anyhow::bail!(
            "metadata ecosystem mismatch: expected {} got {}",
            target.ecosystem.as_str(),
            metadata.ecosystem
        );
    }
    if metadata.locator != target.locator {
        anyhow::bail!(
            "metadata locator mismatch: expected {} got {}",
            target.locator,
            metadata.locator
        );
    }
    if metadata.requested_revision != target.requested_revision {
        anyhow::bail!(
            "metadata requested_revision mismatch: expected {} got {}",
            target.requested_revision,
            metadata.requested_revision
        );
    }
    if metadata.source_fingerprint.is_empty() {
        anyhow::bail!("metadata source_fingerprint is empty");
    }
    if metadata.archive_object_key.is_empty() {
        anyhow::bail!("metadata archive_object_key is empty");
    }
    Ok(())
}

fn archive_directory(source_dir: &Path) -> anyhow::Result<Vec<u8>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(encoder);
    tar.append_dir_all(".", source_dir).with_context(|| {
        format!(
            "failed to archive source directory {}",
            source_dir.display()
        )
    })?;
    let encoder = tar.into_inner().context("failed to finalize tar archive")?;
    encoder
        .finish()
        .context("failed to finalize gzip archive encoding")
}

fn unpack_archive_into_dir(archive_bytes: &[u8], checkout_path: &Path) -> anyhow::Result<()> {
    if checkout_path.exists() {
        anyhow::bail!(
            "refusing to unpack into existing checkout path {}",
            checkout_path.display()
        );
    }

    if let Some(parent) = checkout_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create checkout parent directory {}",
                parent.display()
            )
        })?;
    }
    fs::create_dir_all(checkout_path).with_context(|| {
        format!(
            "failed to create checkout directory {}",
            checkout_path.display()
        )
    })?;

    let cursor = Cursor::new(archive_bytes);
    let decoder = GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(decoder);
    if let Err(err) = archive.unpack(checkout_path) {
        let _ = fs::remove_dir_all(checkout_path);
        return Err(err)
            .with_context(|| format!("failed to unpack archive to {}", checkout_path.display()));
    }

    Ok(())
}

fn normalize_prefix(prefix: &str) -> String {
    prefix.trim_matches('/').to_string()
}

fn with_prefix(prefix: &str, relative: &str) -> String {
    if prefix.is_empty() {
        relative.to_string()
    } else {
        format!("{}/{}", prefix, relative)
    }
}

fn is_not_found(err: &anyhow::Error) -> bool {
    err.downcast_ref::<opendal::Error>()
        .is_some_and(|e| e.kind() == ErrorKind::NotFound)
}
