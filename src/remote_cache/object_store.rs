use anyhow::Context;
use opendal::Operator;
use opendal::services::{Azblob, S3};

use crate::config::{Backend, Config};

pub(super) fn operator_from_config(config: &Config) -> anyhow::Result<Option<Operator>> {
    match config.backend {
        Backend::Local | Backend::AgentFs => Ok(None),
        Backend::S3 => {
            let bucket =
                config.object_store.bucket.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("object_store.bucket must be set for backend=s3")
                })?;
            Ok(Some(build_s3_operator(
                bucket,
                config.object_store.endpoint.as_deref(),
            )?))
        }
        Backend::AzureBlob => {
            let container = config.object_store.bucket.as_deref().ok_or_else(|| {
                anyhow::anyhow!("object_store.bucket must be set for backend=azure_blob")
            })?;
            Ok(Some(build_azblob_operator(
                container,
                config.object_store.endpoint.as_deref(),
            )?))
        }
    }
}

fn build_s3_operator(bucket: &str, endpoint: Option<&str>) -> anyhow::Result<Operator> {
    let mut builder = S3::default().root("/").bucket(bucket);
    let region = std::env::var("PKGREP_OBJECT_STORE_REGION")
        .ok()
        .or_else(|| std::env::var("AWS_REGION").ok())
        .unwrap_or_else(|| "auto".to_string());
    builder = builder.region(&region);

    if let Some(endpoint) = endpoint {
        builder = builder.endpoint(endpoint);
    }
    if let Ok(access_key_id) = std::env::var("PKGREP_OBJECT_STORE_ACCESS_KEY_ID") {
        builder = builder.access_key_id(&access_key_id);
    }
    if let Ok(secret_access_key) = std::env::var("PKGREP_OBJECT_STORE_SECRET_ACCESS_KEY") {
        builder = builder.secret_access_key(&secret_access_key);
    }
    if let Ok(session_token) = std::env::var("PKGREP_OBJECT_STORE_SESSION_TOKEN") {
        builder = builder.session_token(&session_token);
    }

    let operator = Operator::new(builder)
        .context("failed to create S3 operator builder")?
        .finish();
    Ok(operator)
}

fn build_azblob_operator(container: &str, endpoint: Option<&str>) -> anyhow::Result<Operator> {
    let mut builder = Azblob::default().root("/").container(container);

    let endpoint = endpoint
        .map(ToOwned::to_owned)
        .or_else(|| {
            std::env::var("PKGREP_AZURE_ACCOUNT_NAME")
                .ok()
                .map(|name| format!("https://{}.blob.core.windows.net", name))
        })
        .or_else(|| {
            std::env::var("AZURE_STORAGE_ACCOUNT")
                .ok()
                .map(|name| format!("https://{}.blob.core.windows.net", name))
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "object_store.endpoint must be set for backend=azure_blob (or provide account name env vars)"
            )
        })?;
    builder = builder.endpoint(&endpoint);

    if let Ok(account_name) = std::env::var("PKGREP_AZURE_ACCOUNT_NAME") {
        builder = builder.account_name(&account_name);
    } else if let Ok(account_name) = std::env::var("AZURE_STORAGE_ACCOUNT") {
        builder = builder.account_name(&account_name);
    }

    if let Ok(account_key) = std::env::var("PKGREP_AZURE_ACCOUNT_KEY") {
        builder = builder.account_key(&account_key);
    } else if let Ok(account_key) = std::env::var("AZURE_STORAGE_KEY") {
        builder = builder.account_key(&account_key);
    }

    let operator = Operator::new(builder)
        .context("failed to create Azure Blob operator builder")?
        .finish();
    Ok(operator)
}
