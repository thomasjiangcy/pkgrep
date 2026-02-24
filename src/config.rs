use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::PkgrepError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    Local,
    S3,
    AzureBlob,
    AgentFs,
}

impl Backend {
    fn parse(value: &str) -> Result<Self, PkgrepError> {
        match value {
            "local" => Ok(Self::Local),
            "s3" => Ok(Self::S3),
            "azure_blob" => Ok(Self::AzureBlob),
            "agentfs" => Ok(Self::AgentFs),
            other => Err(PkgrepError::InvalidBackend(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
            Self::AzureBlob => "azure_blob",
            Self::AgentFs => "agentfs",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectStoreAuthMode {
    Direct,
    Proxy,
}

impl ObjectStoreAuthMode {
    fn parse(value: &str) -> Result<Self, PkgrepError> {
        match value {
            "direct" => Ok(Self::Direct),
            "proxy" => Ok(Self::Proxy),
            other => Err(PkgrepError::InvalidObjectStoreAuthMode(other.to_string())),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ObjectStoreConfig {
    pub bucket: Option<String>,
    pub prefix: Option<String>,
    pub endpoint: Option<String>,
    pub auth_mode: Option<ObjectStoreAuthMode>,
    pub proxy_identity_header: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub backend: Backend,
    pub cache_dir: PathBuf,
    pub worker_pool_size: usize,
    pub object_store: ObjectStoreConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialConfig {
    backend: Option<String>,
    cache_dir: Option<PathBuf>,
    worker_pool_size: Option<usize>,
    object_store: Option<PartialObjectStoreConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct PartialObjectStoreConfig {
    bucket: Option<String>,
    prefix: Option<String>,
    endpoint: Option<String>,
    auth_mode: Option<String>,
    proxy_identity_header: Option<String>,
}

pub fn load(cwd: &Path) -> Result<Config, PkgrepError> {
    let global_path = global_config_path()?;
    let project_path = cwd.join("pkgrep.toml");

    let global = load_partial_if_exists(&global_path)?;
    let project = load_partial_if_exists(&project_path)?;
    let env = partial_from_env()?;

    merge_config(global, project, env)
}

fn global_config_path() -> Result<PathBuf, PkgrepError> {
    let config_root = config_root_dir().ok_or(PkgrepError::MissingCacheDirectory)?;
    Ok(config_root.join("pkgrep").join("config.toml"))
}

fn default_cache_dir() -> Result<PathBuf, PkgrepError> {
    let home_dir = dirs::home_dir().ok_or(PkgrepError::MissingCacheDirectory)?;
    Ok(home_dir.join(".pkgrep"))
}

fn config_root_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(dirs::config_dir)
}

fn default_worker_pool_size() -> usize {
    match std::thread::available_parallelism() {
        Ok(parallelism) => {
            let base = parallelism.get().saturating_mul(2);
            base.clamp(4, 16)
        }
        Err(_) => 4,
    }
}

fn load_partial_if_exists(path: &Path) -> Result<PartialConfig, PkgrepError> {
    if !path.exists() {
        return Ok(PartialConfig::default());
    }

    let raw = std::fs::read_to_string(path).map_err(|source| PkgrepError::ConfigRead {
        path: path.to_path_buf(),
        source,
    })?;

    toml::from_str(&raw).map_err(|source| PkgrepError::ConfigParse {
        path: path.to_path_buf(),
        source,
    })
}

fn partial_from_env() -> Result<PartialConfig, PkgrepError> {
    let backend = std::env::var("PKGREP_BACKEND").ok();
    let cache_dir = std::env::var("PKGREP_CACHE_DIR").ok().map(PathBuf::from);

    let worker_pool_size = match std::env::var("PKGREP_WORKER_POOL_SIZE") {
        Ok(value) => value.parse::<usize>().ok(),
        Err(_) => None,
    };

    let object_store = Some(PartialObjectStoreConfig {
        bucket: std::env::var("PKGREP_OBJECT_STORE_BUCKET").ok(),
        prefix: std::env::var("PKGREP_OBJECT_STORE_PREFIX").ok(),
        endpoint: std::env::var("PKGREP_OBJECT_STORE_ENDPOINT").ok(),
        auth_mode: std::env::var("PKGREP_OBJECT_STORE_AUTH_MODE").ok(),
        proxy_identity_header: std::env::var("PKGREP_OBJECT_STORE_PROXY_IDENTITY_HEADER").ok(),
    });

    // Parse-only validation that backend/auth mode values are known, but keep layering behavior.
    if let Some(ref raw) = backend {
        let _ = Backend::parse(raw)?;
    }
    if let Some(ref mode) = object_store.as_ref().and_then(|o| o.auth_mode.clone()) {
        let _ = ObjectStoreAuthMode::parse(mode)?;
    }

    Ok(PartialConfig {
        backend,
        cache_dir,
        worker_pool_size,
        object_store,
    })
}

fn merge_config(
    global: PartialConfig,
    project: PartialConfig,
    env: PartialConfig,
) -> Result<Config, PkgrepError> {
    let backend_raw = env
        .backend
        .or(project.backend)
        .or(global.backend)
        .unwrap_or_else(|| "local".to_string());
    let backend = Backend::parse(&backend_raw)?;

    let cache_dir = env
        .cache_dir
        .or(project.cache_dir)
        .or(global.cache_dir)
        .map(Ok)
        .unwrap_or_else(default_cache_dir)?;

    let worker_pool_size = env
        .worker_pool_size
        .or(project.worker_pool_size)
        .or(global.worker_pool_size)
        .unwrap_or_else(default_worker_pool_size);

    if worker_pool_size < 1 {
        return Err(PkgrepError::InvalidWorkerPoolSize(worker_pool_size));
    }

    let global_os = global.object_store.unwrap_or_default();
    let project_os = project.object_store.unwrap_or_default();
    let env_os = env.object_store.unwrap_or_default();

    let auth_mode = env_os
        .auth_mode
        .as_deref()
        .map(ObjectStoreAuthMode::parse)
        .transpose()?
        .or(project_os
            .auth_mode
            .as_deref()
            .map(ObjectStoreAuthMode::parse)
            .transpose()?)
        .or(global_os
            .auth_mode
            .as_deref()
            .map(ObjectStoreAuthMode::parse)
            .transpose()?);

    let object_store = ObjectStoreConfig {
        bucket: env_os.bucket.or(project_os.bucket).or(global_os.bucket),
        prefix: env_os.prefix.or(project_os.prefix).or(global_os.prefix),
        endpoint: env_os
            .endpoint
            .or(project_os.endpoint)
            .or(global_os.endpoint),
        auth_mode,
        proxy_identity_header: env_os
            .proxy_identity_header
            .or(project_os.proxy_identity_header)
            .or(global_os.proxy_identity_header),
    };

    Ok(Config {
        backend,
        cache_dir,
        worker_pool_size,
        object_store,
    })
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_partial(backend: Option<&str>, worker_pool_size: Option<usize>) -> PartialConfig {
        PartialConfig {
            backend: backend.map(str::to_string),
            worker_pool_size,
            ..PartialConfig::default()
        }
    }

    #[test]
    fn project_overrides_global_and_env_overrides_project() {
        let global = make_partial(Some("local"), Some(4));
        let project = make_partial(Some("s3"), Some(8));
        let env = make_partial(Some("azure_blob"), None);

        let cfg = merge_config(global, project, env).expect("merge");
        assert_eq!(cfg.backend, Backend::AzureBlob);
        assert_eq!(cfg.worker_pool_size, 8);
    }

    #[test]
    fn defaults_worker_pool_and_backend() {
        let cfg = merge_config(
            PartialConfig::default(),
            PartialConfig::default(),
            PartialConfig::default(),
        )
        .expect("merge");

        assert_eq!(cfg.backend, Backend::Local);
        assert!(cfg.worker_pool_size >= 1);
    }

    #[test]
    fn invalid_worker_pool_size_fails() {
        let global = make_partial(None, Some(0));
        let err = merge_config(global, PartialConfig::default(), PartialConfig::default())
            .expect_err("should fail");

        assert!(matches!(err, PkgrepError::InvalidWorkerPoolSize(0)));
    }
}
