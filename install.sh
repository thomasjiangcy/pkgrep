#!/usr/bin/env sh
set -eu

BIN_NAME="pkgrep"
VERSION="${PKGREP_VERSION:-latest}"
INSTALL_DIR="${PKGREP_INSTALL_DIR:-${HOME}/.local/bin}"
DEFAULT_REPO="thomasjiangcy/pkgrep"
REPO="${PKGREP_REPO:-${DEFAULT_REPO}}"

usage() {
  cat <<'EOF'
Install pkgrep from GitHub Releases.

Usage:
  install.sh [--repo <owner/repo>] [--version <tag|latest>] [--install-dir <path>]

Options:
  --repo        GitHub repository in owner/repo format (default: thomasjiangcy/pkgrep)
  --version     Release tag (for example v0.1.0) or "latest" (default)
  --install-dir Destination directory for the binary (default: ~/.local/bin)
  -h, --help    Show this help

Environment variables:
  PKGREP_REPO
  PKGREP_VERSION
  PKGREP_INSTALL_DIR
EOF
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "${os}-${arch}" in
    Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Darwin-arm64|Darwin-aarch64) echo "aarch64-apple-darwin" ;;
    *)
      echo "unsupported platform: ${os}-${arch}" >&2
      exit 1
      ;;
  esac
}

http_get_to_file() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "${out}" "${url}"
    return 0
  fi
  if command -v wget >/dev/null 2>&1; then
    wget -q -O "${out}" "${url}"
    return 0
  fi
  echo "missing downloader: install curl or wget" >&2
  exit 1
}

http_get_text() {
  url="$1"
  out="$(mktemp)"
  http_get_to_file "${url}" "${out}"
  cat "${out}"
  rm -f "${out}"
}

resolve_version() {
  if [ "${VERSION}" != "latest" ]; then
    printf '%s\n' "${VERSION}"
    return 0
  fi

  api_url="https://api.github.com/repos/${REPO}/releases/latest"
  tag="$(http_get_text "${api_url}" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
  if [ -z "${tag}" ]; then
    echo "failed to resolve latest release tag from ${api_url}" >&2
    exit 1
  fi
  printf '%s\n' "${tag}"
}

verify_sha256() {
  archive="$1"
  checksum_file="$2"

  if command -v shasum >/dev/null 2>&1; then
    (cd "$(dirname "${archive}")" && shasum -a 256 -c "$(basename "${checksum_file}")")
    return 0
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$(dirname "${archive}")" && sha256sum -c "$(basename "${checksum_file}")")
    return 0
  fi

  echo "warning: shasum/sha256sum not found; skipping checksum verification" >&2
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      if [ "$#" -lt 2 ]; then
        echo "missing value for --repo" >&2
        exit 1
      fi
      REPO="${2:-}"
      shift 2
      ;;
    --version)
      if [ "$#" -lt 2 ]; then
        echo "missing value for --version" >&2
        exit 1
      fi
      VERSION="${2:-}"
      shift 2
      ;;
    --install-dir)
      if [ "$#" -lt 2 ]; then
        echo "missing value for --install-dir" >&2
        exit 1
      fi
      INSTALL_DIR="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [ -z "${REPO}" ]; then
  echo "missing repository: set --repo <owner/repo> or PKGREP_REPO" >&2
  exit 1
fi

TARGET="$(detect_target)"
RESOLVED_VERSION="$(resolve_version)"
ARCHIVE="${BIN_NAME}-${RESOLVED_VERSION}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${RESOLVED_VERSION}"
ARCHIVE_URL="${BASE_URL}/${ARCHIVE}"
CHECKSUM_URL="${ARCHIVE_URL}.sha256"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' 0 2 3 15

ARCHIVE_PATH="${TMP_DIR}/${ARCHIVE}"
CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"

echo "installing ${BIN_NAME} ${RESOLVED_VERSION} (${TARGET}) from ${REPO}"
http_get_to_file "${ARCHIVE_URL}" "${ARCHIVE_PATH}"

if http_get_to_file "${CHECKSUM_URL}" "${CHECKSUM_PATH}"; then
  verify_sha256 "${ARCHIVE_PATH}" "${CHECKSUM_PATH}"
else
  echo "warning: checksum file unavailable; continuing without checksum verification" >&2
fi

tar -xzf "${ARCHIVE_PATH}" -C "${TMP_DIR}" "${BIN_NAME}"
mkdir -p "${INSTALL_DIR}"

if command -v install >/dev/null 2>&1; then
  install -m 0755 "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
else
  cp "${TMP_DIR}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
  chmod 0755 "${INSTALL_DIR}/${BIN_NAME}"
fi

echo "installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"
case ":${PATH:-}:" in
  *":${INSTALL_DIR}:"*) ;;
  *) echo "note: ${INSTALL_DIR} is not on PATH" ;;
esac
