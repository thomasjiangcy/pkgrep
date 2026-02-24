# pkgrep

`pkgrep` helps developers and coding agents fetch dependency source code into a local cache and expose it in a project via symlinks for easy source traversal.

## Disclosure

This project is currently written 100% by Codex (an AI coding agent), without human-written code contributions.

Do not use this project if you are not comfortable adopting and running code that is fully agent-written.

## Why This Exists

Modern dependency managers often install packaged artifacts (compiled bundles, generated files, wheels, etc.), not easy-to-traverse source trees for a specific dependency version.

That creates a gap for agent-assisted development:

- Coding agents can infer intent faster when they can inspect real upstream implementation code.
- Developers need deterministic, reusable local source snapshots across projects.
- Teams need a simple workflow to link dependency source into a project without manual cloning and ad-hoc scripts.

`pkgrep` addresses this by caching dependency source centrally by version/fingerprint, linking it into each project in a consistent location, and tracking references so stale cache entries can be pruned safely.

## Key Features

- 📦 Centrally managed dependency source cache with symlinked project links for efficient storage reuse
- 🤖 Non-interactive CLI by default for agent-friendly automation
- ☁️ Remote cache support via object stores (`s3`, `azure_blob`)

## Installation

### Homebrew

```bash
brew tap thomasjiangcy/homebrew-tap
brew install pkgrep
```

### GitHub Releases

Download the archive for your platform from the project Releases page and place `pkgrep` on your `PATH`.

### Direct Download (`curl` / `wget`)

One-line install with `curl`:

```bash
curl -fsSL https://raw.githubusercontent.com/thomasjiangcy/pkgrep/main/install.sh | sh
```

One-line install with `wget`:

```bash
wget -qO- https://raw.githubusercontent.com/thomasjiangcy/pkgrep/main/install.sh | sh
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/thomasjiangcy/pkgrep/main/install.sh | \
  sh -s -- --version v0.1.0
```

Install options:

```bash
./install.sh --help
```

Notes:

- `install.sh` auto-detects platform target and installs to `${HOME}/.local/bin` by default.
- It resolves `--version latest` via GitHub Releases API.
- It verifies archive checksum when `.sha256` is available.
- Override release source for forks with `--repo <owner/repo>`.

### From source

```bash
git clone https://github.com/thomasjiangcy/pkgrep.git
cd pkgrep
cargo install --path .
```

### Verify

```bash
pkgrep --help
```

## Usage

`pkgrep` currently exposes these commands:

- `pkgrep pull [dep-spec ...]`
- `pkgrep path <dep-spec>`
- `pkgrep remove <dep-spec ...> [--yes]`
- `pkgrep cache hydrate [dep-spec ...]`
- `pkgrep cache clean [--yes]`
- `pkgrep cache prune [--yes]`

Examples:

```bash
# Pull explicit git dependency source
pkgrep pull git:https://github.com/facebook/react.git@v18.3.1

# Pull npm package source by package version
pkgrep pull npm:zod@3.23.8

# Pull package source using implicit ecosystem inference from project lockfile(s)
# (works only when exactly one supported ecosystem is detected in cwd)
pkgrep pull zod@3.23.8

# Pull npm package source using registry latest tag
pkgrep pull npm:react

# Pull PyPI package source by package version
pkgrep pull pypi:requests@2.32.3

# Pull PyPI package source using registry latest version
pkgrep pull pypi:fastapi

# Pull explicit git dependency source when tag/revision contains '@'
pkgrep pull 'git:https://github.com/facebook/react.git@eslint-plugin-react-hooks@5.0.0'
# equivalent unambiguous form:
pkgrep pull 'git:https://github.com/facebook/react.git#eslint-plugin-react-hooks@5.0.0'

# Pull from project files in current directory
# (currently auto-detects package-lock.json and uv.lock, and only pulls entries with git source hints)
pkgrep pull

# Resolve the linked project path for a dep
pkgrep path git:https://github.com/facebook/react.git@v18.3.1

# Remove project links (requires --yes)
pkgrep remove git:https://github.com/facebook/react.git@v18.3.1 --yes

# Hydrate local cache from remote object store
pkgrep cache hydrate git:https://github.com/facebook/react.git@v18.3.1

# Clean local cache (requires --yes)
pkgrep cache clean --yes

# Prune unreferenced cached checkouts/mirrors (dry-run by default)
pkgrep cache prune
pkgrep cache prune --yes
```

Current behavior:

- `remove`, `cache clean`, and `cache prune` are no-op unless `--yes` is provided.
- `cache hydrate` requires a remote backend (`s3` or `azure_blob`) and only hydrates git-backed sources from object storage.
- `pull` supports:
  - explicit git specs (`git:<url>@<revision>` or `git:<url>#<revision>`)
  - npm package specs (`npm:<name>` / `npm:<name>@<version>`) resolved via npm metadata
  - pypi package specs (`pypi:<name>` / `pypi:<name>@<version>`) resolved via PyPI metadata
  - shorthand package specs (`<name>` / `<name>@<version>`) when exactly one supported ecosystem is inferred from project lockfiles in cwd
- `path` currently supports git-backed specs and returns the linked project path when present.
- Git dep specs accept both `git:<url>@<revision>` and `git:<url>#<revision>`.
- Project links are human-readable under `.pkgrep/deps/...`; internal cache keys remain normalized for safety/determinism.
- With remote backend configured, `pull` attempts remote hydrate first; if missing, it fetches from Git and then publishes to remote cache.
- `cache prune` reconciles stale project references from the global index, then prunes unreferenced local checkouts and git mirrors.
- `cache prune` dry-run output shows human-readable dependency identities plus filesystem paths.

## Local Index Files

`pkgrep` maintains two local JSON index files:

- Project manifest: `.pkgrep/manifest.json`
- Global reverse index: `<cache_dir>/index/project_refs.json` (default: `~/.pkgrep/index/project_refs.json`)

Project manifest entry example:

```json
{
  "schema_version": 1,
  "entries": {
    "git:https://github.com/facebook/react.git@eslint-plugin-react-hooks@5.0.0": {
      "link_path": ".pkgrep/deps/git/github.com/facebook/react.git@eslint-plugin-react-hooks@5.0.0",
      "cache_key": "git/b64_.../eslint-plugin-react-hooks@5.0.0/f1338f..."
    }
  }
}
```

Global reverse index entry example:

```json
{
  "schema_version": 1,
  "entries": {
    "git/b64_.../eslint-plugin-react-hooks@5.0.0/f1338f...": {
      "dep_spec": "git:https://github.com/facebook/react.git@eslint-plugin-react-hooks@5.0.0",
      "checkout_path": "/home/user/.pkgrep/sources/git/b64_.../eslint-plugin-react-hooks@5.0.0/f1338f...",
      "projects": [
        "/home/user/projects/my-app"
      ]
    }
  }
}
```

## Configuration

Config precedence:

1. Environment variables
2. Project config: `<project>/pkgrep.toml`
3. Global config: `${XDG_CONFIG_HOME:-~/.config}/pkgrep/config.toml`
4. Defaults

Example `pkgrep.toml`:

```toml
backend = "s3" # local | s3 | azure_blob
cache_dir = "/tmp/pkgrep-cache"
worker_pool_size = 8

[object_store]
auth_mode = "direct" # direct | proxy
endpoint = "http://127.0.0.1:8333"
bucket = "pkgrep-cache"
prefix = "v1/dev"
proxy_identity_header = "x-workload-jwt" # proxy mode only
```

Azure note:

- `object_store.bucket` maps to Azure Blob **container** name for `backend = "azure_blob"`.
- Azure Blob is not S3-compatible; use Azurite for local Azure testing.

Object store credential env vars currently supported:

- S3: `PKGREP_OBJECT_STORE_ACCESS_KEY_ID`, `PKGREP_OBJECT_STORE_SECRET_ACCESS_KEY`, `PKGREP_OBJECT_STORE_SESSION_TOKEN`, optional `PKGREP_OBJECT_STORE_REGION`.
- Azure Blob: `PKGREP_AZURE_ACCOUNT_NAME`, `PKGREP_AZURE_ACCOUNT_KEY` (or `AZURE_STORAGE_ACCOUNT`, `AZURE_STORAGE_KEY`).

Proxy mode:

- `auth_mode = "proxy"` can be used when outbound requests are routed through an egress/signing proxy.
- Current implementation does not add custom auth headers itself; proxy integration should inject auth at the network/proxy layer before requests reach object storage.

Worker pool default:

- `max(4, min(16, 2 * available_parallelism))`
- default cache dir: `~/.pkgrep` (override with `PKGREP_CACHE_DIR` or config `cache_dir`)

Logging:

- default: `warn` with concise, human-readable formatting (no timestamp noise)
- CLI override: `--verbose` (uses `debug`)
- env override: `RUST_LOG=debug`

Registry metadata endpoint overrides (for private mirrors/airgapped environments):

- `PKGREP_NPM_REGISTRY_URL` (default: `https://registry.npmjs.org`)
- `PKGREP_PYPI_REGISTRY_URL` (default: `https://pypi.org/pypi`)

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the full contributor guide.

Prerequisites:

- Rust toolchain from `rust-toolchain.toml`
- `mise` (required for project tooling)
- `just` (required task runner)
- `lefthook` (required for git hook checks)

### Tooling via mise

```bash
mise install
just hooks-install
```

Common development commands:

```bash
just fmt
just lint
just test
just ci
just hooks-run
```

Git hooks:

- `pre-commit`: no-mocks policy + `cargo fmt --check`
- `pre-push`: clippy (`-D warnings`) + full test suite

### Maintainer Release Flow

1. Create and push a version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

2. GitHub Actions workflow `Release` builds and uploads platform artifacts to the GitHub Release.
3. GitHub Actions workflow `Homebrew Tap Publish` updates `Formula/pkgrep.rb` in your tap repository.

Required repo settings for Homebrew publish:

- Repository variable: `HOMEBREW_TAP_REPOSITORY` (example: `owner/homebrew-tap`)
- Repository secret: `HOMEBREW_TAP_GITHUB_TOKEN` (token with push access to the tap repo)

### Remote E2E tests (Docker + testcontainers)

Remote cache E2E tests now self-manage infrastructure with `testcontainers`:

- SeaweedFS + `aws-cli` bucket bootstrap for S3 tests
- Azurite + `azure-cli` container bootstrap for Azure Blob tests

Requirements:

- Docker daemon available and running

Run remote S3 E2E:

```bash
just test-remote-s3
```

Run remote Azure Blob E2E:

```bash
just test-remote-azure
```

Run both remote E2E suites:

```bash
just test-remote-all
```

Note:

- `tests/remote_cache_e2e.rs` reads the SeaweedFS S3 identity fixture from `.dev/seaweedfs/s3.json`.

## License

[MIT](./LICENSE)
