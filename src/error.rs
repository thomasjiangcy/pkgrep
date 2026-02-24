use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PkgrepError {
    #[error("invalid worker_pool_size: {0} (must be >= 1)")]
    InvalidWorkerPoolSize(usize),

    #[error("invalid backend: {0} (expected one of: local, s3, azure_blob, agentfs)")]
    InvalidBackend(String),

    #[error("invalid object store auth mode: {0} (expected one of: direct, proxy)")]
    InvalidObjectStoreAuthMode(String),

    #[error("failed to read config file {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config file {path}: {source}")]
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("unable to derive a cache directory from the current environment")]
    MissingCacheDirectory,
}
