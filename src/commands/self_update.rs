use std::ffi::OsStr;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const DEFAULT_RELEASE_REPOSITORY: &str = "thomasjiangcy/pkgrep";
const ENV_RELEASE_REPOSITORY: &str = "PKGREP_SELF_UPDATE_REPO";
const BINARY_NAME: &str = "pkgrep";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

pub(super) fn run_self_update() -> anyhow::Result<()> {
    let release_repository = release_repository();
    let current_exe =
        std::env::current_exe().context("failed to resolve current executable path")?;
    let install_path = current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.clone());

    if is_homebrew_managed_path(&install_path) {
        anyhow::bail!(
            "self-update is disabled for Homebrew-managed installs; run: brew upgrade pkgrep"
        );
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let target = detect_release_target()?;
    let http_client = build_http_client()?;

    println!(
        "Self update: checking latest release from {}",
        release_repository
    );
    let release = fetch_latest_release(&http_client, &release_repository)?;
    let latest_tag = release.tag_name.trim().to_string();
    if latest_tag.is_empty() {
        anyhow::bail!("latest release tag from GitHub was empty");
    }

    if normalize_version(current_version) == normalize_version(&latest_tag) {
        println!("Self update: already up to date ({})", latest_tag);
        return Ok(());
    }

    println!(
        "Self update: updating {} -> {}",
        current_version, latest_tag
    );

    let archive_name = format!("{}-{}-{}.tar.gz", BINARY_NAME, latest_tag, target);
    let checksum_name = format!("{}.sha256", archive_name);

    let archive_url = find_asset_download_url(&release.assets, &archive_name).ok_or_else(|| {
        anyhow::anyhow!(
            "release asset not found for target '{}': {}",
            target,
            archive_name
        )
    })?;
    let checksum_url =
        find_asset_download_url(&release.assets, &checksum_name).ok_or_else(|| {
            anyhow::anyhow!(
                "release checksum asset not found for target '{}': {}",
                target,
                checksum_name
            )
        })?;

    println!("Self update: downloading {}", archive_name);
    let archive_bytes = download_bytes(&http_client, archive_url)
        .with_context(|| format!("failed to download {}", archive_name))?;

    println!("Self update: downloading {}", checksum_name);
    let checksum_text = download_text(&http_client, checksum_url)
        .with_context(|| format!("failed to download {}", checksum_name))?;

    verify_archive_sha256(&archive_bytes, &checksum_text, &archive_name)?;

    println!("Self update: extracting binary");
    let binary_bytes = extract_binary_from_archive(&archive_bytes, BINARY_NAME)?;

    println!("Self update: installing to {}", install_path.display());
    replace_binary_atomically(&install_path, &binary_bytes)?;

    println!(
        "Self update: success ({} -> {})",
        current_version, latest_tag
    );
    Ok(())
}

fn release_repository() -> String {
    match std::env::var(ENV_RELEASE_REPOSITORY) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => DEFAULT_RELEASE_REPOSITORY.to_string(),
    }
}

fn detect_release_target() -> anyhow::Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        (os, arch) => anyhow::bail!("unsupported platform for self-update: {}-{}", os, arch),
    }
}

fn build_http_client() -> anyhow::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("pkgrep/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()
        .context("failed to build HTTP client for self-update")
}

fn fetch_latest_release(
    http_client: &reqwest::blocking::Client,
    release_repository: &str,
) -> anyhow::Result<GitHubRelease> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        release_repository
    );
    let response = http_client
        .get(&url)
        .send()
        .with_context(|| format!("failed to fetch latest release metadata from {}", url))?
        .error_for_status()
        .with_context(|| format!("GitHub latest release request failed for {}", url))?;

    response
        .json::<GitHubRelease>()
        .with_context(|| format!("failed to parse latest release metadata from {}", url))
}

fn find_asset_download_url<'a>(
    assets: &'a [GitHubReleaseAsset],
    asset_name: &str,
) -> Option<&'a str> {
    assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .map(|asset| asset.browser_download_url.as_str())
}

fn download_bytes(http_client: &reqwest::blocking::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let response = http_client
        .get(url)
        .send()
        .with_context(|| format!("failed to download {}", url))?
        .error_for_status()
        .with_context(|| format!("download request failed for {}", url))?;

    response
        .bytes()
        .with_context(|| format!("failed to read download payload from {}", url))
        .map(|bytes| bytes.to_vec())
}

fn download_text(http_client: &reqwest::blocking::Client, url: &str) -> anyhow::Result<String> {
    let response = http_client
        .get(url)
        .send()
        .with_context(|| format!("failed to download {}", url))?
        .error_for_status()
        .with_context(|| format!("download request failed for {}", url))?;

    response
        .text()
        .with_context(|| format!("failed to read text payload from {}", url))
}

fn verify_archive_sha256(
    archive_bytes: &[u8],
    checksum_text: &str,
    archive_name: &str,
) -> anyhow::Result<()> {
    let expected = parse_sha256_for_archive(checksum_text, archive_name).ok_or_else(|| {
        anyhow::anyhow!(
            "checksum entry for '{}' not found in downloaded checksum file",
            archive_name
        )
    })?;

    if !is_valid_sha256_hex(&expected) {
        anyhow::bail!(
            "invalid checksum digest format for '{}': expected 64-char hex",
            archive_name
        );
    }

    let actual = compute_sha256_hex(archive_bytes);
    if actual != expected {
        anyhow::bail!(
            "checksum mismatch for '{}': expected {}, got {}",
            archive_name,
            expected,
            actual
        );
    }

    Ok(())
}

fn parse_sha256_for_archive(checksum_text: &str, archive_name: &str) -> Option<String> {
    for line in checksum_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut fields = trimmed.split_whitespace();
        let digest = fields.next()?;
        let file_token = fields.next()?;
        let normalized_file = file_token.trim_start_matches('*');

        if normalized_file == archive_name || normalized_file.ends_with(archive_name) {
            return Some(digest.to_ascii_lowercase());
        }
    }

    None
}

fn is_valid_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn compute_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{:x}", digest)
}

fn extract_binary_from_archive(archive_bytes: &[u8], binary_name: &str) -> anyhow::Result<Vec<u8>> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = tar::Archive::new(decoder);

    for entry in archive
        .entries()
        .context("failed to read archive entries")?
    {
        let mut entry = entry.context("failed to read archive entry")?;
        let path = entry
            .path()
            .context("failed to inspect archive entry path")?;

        if path
            .file_name()
            .is_some_and(|file_name| file_name == OsStr::new(binary_name))
        {
            let mut buffer = Vec::new();
            entry
                .read_to_end(&mut buffer)
                .context("failed to read binary content from archive")?;
            if buffer.is_empty() {
                anyhow::bail!("archive contained empty binary payload");
            }
            return Ok(buffer);
        }
    }

    anyhow::bail!("archive did not contain '{}' binary", binary_name)
}

fn replace_binary_atomically(install_path: &Path, binary_bytes: &[u8]) -> anyhow::Result<()> {
    if !install_path.exists() {
        anyhow::bail!(
            "cannot self-update because current binary path does not exist: {}",
            install_path.display()
        );
    }

    let parent = install_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "failed to resolve parent directory for binary path {}",
            install_path.display()
        )
    })?;

    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;

    let unique = unique_suffix();
    let temp_path = parent.join(format!(".pkgrep-self-update-{}.new", unique));
    let backup_path = parent.join(format!(".pkgrep-self-update-{}.bak", unique));

    fs::write(&temp_path, binary_bytes)
        .with_context(|| format!("failed to write staged binary at {}", temp_path.display()))?;
    set_executable_permissions(&temp_path)?;

    fs::rename(install_path, &backup_path).with_context(|| {
        format!(
            "failed to move current binary into backup location {}",
            backup_path.display()
        )
    })?;

    if let Err(error) = fs::rename(&temp_path, install_path) {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::rename(&backup_path, install_path);
        return Err(error).with_context(|| {
            format!(
                "failed to move new binary into place at {}",
                install_path.display()
            )
        });
    }

    if let Err(error) = fs::remove_file(&backup_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).with_context(|| {
            format!(
                "failed to remove self-update backup file {}",
                backup_path.display()
            )
        });
    }

    Ok(())
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set executable permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), timestamp)
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

fn is_homebrew_managed_path(path: &Path) -> bool {
    let mut saw_cellar = false;

    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if !saw_cellar {
            if name == "Cellar" {
                saw_cellar = true;
            }
            continue;
        }

        return name == BINARY_NAME;
    }

    false
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;

    use crate::commands::self_update::{
        BINARY_NAME, GitHubReleaseAsset, compute_sha256_hex, extract_binary_from_archive,
        find_asset_download_url, is_homebrew_managed_path, normalize_version,
        parse_sha256_for_archive,
    };

    #[test]
    fn normalize_version_strips_v_prefix() {
        assert_eq!(normalize_version("v0.2.0"), "0.2.0");
        assert_eq!(normalize_version("0.2.0"), "0.2.0");
    }

    #[test]
    fn detects_homebrew_managed_paths() {
        assert!(is_homebrew_managed_path(Path::new(
            "/opt/homebrew/Cellar/pkgrep/0.2.0/bin/pkgrep"
        )));
        assert!(!is_homebrew_managed_path(Path::new(
            "/usr/local/bin/pkgrep"
        )));
    }

    #[test]
    fn finds_release_asset_download_url_by_exact_name() {
        let assets = vec![
            GitHubReleaseAsset {
                name: "pkgrep-v0.2.0-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux.tar.gz".to_string(),
            },
            GitHubReleaseAsset {
                name: "pkgrep-v0.2.0-aarch64-apple-darwin.tar.gz".to_string(),
                browser_download_url: "https://example.com/macos.tar.gz".to_string(),
            },
        ];

        let url = find_asset_download_url(&assets, "pkgrep-v0.2.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(url, Some("https://example.com/macos.tar.gz"));
    }

    #[test]
    fn parses_sha256_entry_for_archive() {
        let checksum = "4f13f149b74e1ea2a0eb7abde2d8d4f2f7b1314ed57eb0317f12f3f432aa0f72  pkgrep-v0.2.0-aarch64-apple-darwin.tar.gz\n";

        let parsed =
            parse_sha256_for_archive(checksum, "pkgrep-v0.2.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(
            parsed.as_deref(),
            Some("4f13f149b74e1ea2a0eb7abde2d8d4f2f7b1314ed57eb0317f12f3f432aa0f72")
        );
    }

    #[test]
    fn extracts_pkgrep_binary_from_tar_gz_archive() {
        let mut tar_payload = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_payload);
            let payload = b"binary-data";
            let mut header = tar::Header::new_gnu();
            header.set_path(BINARY_NAME).expect("set path");
            header.set_mode(0o755);
            header.set_size(payload.len() as u64);
            header.set_cksum();
            builder.append(&header, &payload[..]).expect("append entry");
            builder.finish().expect("finish tar builder");
        }

        let mut gz_payload = Vec::new();
        {
            let mut encoder =
                flate2::write::GzEncoder::new(&mut gz_payload, flate2::Compression::default());
            encoder.write_all(&tar_payload).expect("write tar payload");
            encoder.finish().expect("finish gzip encoder");
        }

        let extracted = extract_binary_from_archive(&gz_payload, BINARY_NAME).expect("extract");
        assert_eq!(extracted, b"binary-data");
    }

    #[test]
    fn computes_sha256_for_bytes() {
        assert_eq!(
            compute_sha256_hex(b"pkgrep"),
            "7c4a9fd86307ac1c7e673feb6c384d13721b56366218aceec4d9b594dd97d437"
        );
    }
}
